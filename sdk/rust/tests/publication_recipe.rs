//! Shared owned recipes reach the real retained compiler and preserve native contracts.

use std::fs;

use gazette_sdk::{authored, build, source, workspace::Key};
use git2::{IndexAddOption, Repository, Signature};
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

#[path = "publication_recipe/social_cards.rs"]
mod social_cards;

#[path = "publication_recipe/documentation_catalog.rs"]
mod documentation_catalog;

struct Fixture {
    directory: TempDir,
    workspace: String,
}

impl Fixture {
    fn new(names: &[&str]) -> Self {
        Self::with_drafts(names, false)
    }

    fn with_drafts(names: &[&str], drafts: bool) -> Self {
        Self::with_pages(names, drafts, false)
    }

    fn with_pages(names: &[&str], drafts: bool, retained_page: bool) -> Self {
        let directory = tempdir().expect("disposable exact sources");
        let root = directory.path();
        fs::create_dir_all(root.join("theme/templates")).expect("Theme directory");
        fs::write(
            root.join("theme/theme.toml"),
            "package='theme:one'\nnamespace='one'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html'}\n",
        )
        .expect("manifest");
        fs::write(
            root.join("theme/templates/page.html"),
            "<h1>{{ content.title }}</h1>{{ body_html | safe }}",
        )
        .expect("native template");
        fs::write(
            root.join("theme/templates/alias.html"),
            "<a href=\"{{ route.canonical_url }}\">Moved</a>",
        )
        .expect("native alias template");
        for name in names {
            fs::create_dir(root.join(name)).expect("authored source");
            let lifecycle = if drafts && *name == "one" {
                "draft"
            } else {
                "published"
            };
            fs::write(root.join(name).join("page.md"), format!("+++\n[content]\nid='content:{name}'\nsite='site:{name}'\ntitle='{name}'\nlifecycle='{lifecycle}'\nroute={{path='/',aliases=['/old.html']}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n# Retained {name}\n")).expect("authored bytes");
            if retained_page {
                fs::write(root.join(name).join("retained.md"), format!("+++\n[content]\nid='content:{name}-retained'\nsite='site:{name}'\ntitle='Retained {name}'\nlifecycle='published'\nroute={{path='/retained/'}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n# Retained published {name}\n")).expect("separate published page at exact source commit");
            }
        }
        let repository = Repository::init(root).expect("native fixture Git");
        let mut index = repository.index().expect("index");
        index
            .add_all(
                ["theme"].into_iter().chain(names.iter().copied()),
                IndexAddOption::DEFAULT,
                None,
            )
            .expect("selected native files");
        index.write().expect("fixture index");
        let tree = repository
            .find_tree(index.write_tree().expect("tree OID"))
            .expect("tree");
        let signature =
            Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("fixed signature");
        let commit = repository
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .expect("local commit");
        let mut workspace = String::from(
            "[composition]\ntheme={package='theme:one'}\n[[principals]]\nid='owner:one'\nkind='person'\n",
        );
        for name in names {
            workspace.push_str(&format!(
                "[[sites]]\nid='site:{name}'\npublisher='owner:one'\ndeployable=false\n"
            ));
        }
        workspace.push_str("[[brands]]\nid='brand:one'\nname='Recipe fixture'\n");
        for name in names {
            workspace.push_str(&format!("[[brands.publications]]\nsite='site:{name}'\npublic_base_url='https://{name}.example/mount/'\nauthored=[{{source='source:{name}',target='{name}'}}]\n"));
        }
        for name in ["theme"].into_iter().chain(names.iter().copied()) {
            workspace.push_str(&format!("[[sources]]\nid='source:{name}'\nlocation={{kind='repository',root={:?},commit='{commit}',subdirectory='{name}'}}\n", root.to_str().expect("UTF-8 root")));
        }
        workspace.push_str(
            "[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n",
        );
        fs::write(root.join("gazette.toml"), "includes=['workspace.toml']\n")
            .expect("explicit root workspace document");
        fs::write(root.join("workspace.toml"), &workspace)
            .expect("explicit included workspace document");
        Self {
            directory,
            workspace,
        }
    }
}

#[test]
fn modular_inspection_retains_configuration_and_workspace_relative_source_roots() {
    let fixture = Fixture::new(&["one"]);
    let root = fixture.directory.path();
    let source = fixture
        .workspace
        .replace(&format!("{:?}", root.to_str().expect("UTF-8 root")), "'.'");
    fs::write(root.join("workspace.toml"), source).expect("relative source bindings");
    fs::write(root.join("unselected.toml"), "invalid and unselected").expect("unselected sibling");
    let selection = build::WorkspaceSource::Selected(build::WorkspaceInput::Local {
        root: root.to_owned(),
        workspace: "gazette.toml".parse().expect("root path"),
        limits: Default::default(),
    });
    let site = "site:one".parse().expect("site key");
    let composition = build::acquire_selected_composition(
        &selection,
        &site,
        Default::default(),
        build::PublicationSetAcquisitionLimits::default().sources,
    )
    .expect("modular composition");
    let evidence =
        serde_json::to_value(composition.configuration_evidence()).expect("configuration evidence");
    assert_eq!(evidence["kind"], "documents");
    assert_eq!(
        evidence["documents"].as_array().expect("documents").len(),
        2
    );
    let metadata = composition
        .check_metadata(Default::default())
        .expect("metadata");
    build::compile_templates(&metadata, Default::default()).expect("native templates");
    let publication = build::acquire_selected_publication(
        &selection,
        &site,
        Default::default(),
        build::PublicationSetAcquisitionLimits::default().sources,
    )
    .expect("modular publication");
    let parsed = build::parse_publication(&publication, Default::default()).expect("corpus");
    parsed
        .resolve(Default::default())
        .map(|resolved| {
            let inspection = resolved.inspect();
            assert_eq!(inspection.authoring_sources.len(), 1);
            let source = &inspection.authoring_sources[0];
            assert_eq!(source.source.as_str(), "source:one");
            assert_eq!(source.entry, "page.md");
            assert_eq!(source.path.as_str(), "one/page.md");
        })
        .expect("resolved publication");
    fs::remove_file(root.join("workspace.toml")).expect("remove required include");
    assert!(
        build::acquire_selected_composition(
            &selection,
            &site,
            Default::default(),
            build::PublicationSetAcquisitionLimits::default().sources,
        )
        .is_err(),
        "missing includes must not reuse an earlier successful workspace"
    );
}

#[test]
fn configured_build_defaults_reach_native_acquisition_selection_and_output() {
    for policy in ["include", "exclude"] {
        let fixture = Fixture::with_drafts(&["one", "two"], true);
        let workspace = format!(
            "[build]\nsites=['site:two','site:one']\nlocal_drafts='{policy}'\n{}",
            fixture.workspace
        );
        let inputs = build::acquire_configured_publications(
            &workspace,
            Default::default(),
            Default::default(),
        )
        .expect("configured native site acquisition");
        assert_eq!(
            inputs.build_selection().origin,
            gazette_sdk::workspace::BuildOrigin::Workspace
        );
        assert_eq!(inputs.build_selection().sites, [key("two"), key("one")]);
        let choices = recipe(&["two", "one"], None);
        let rendered = build::render_publications(
            &inputs,
            choices.options(Default::default()),
            Default::default(),
        )
        .expect("same shared compiler and Theme");
        assert_eq!(rendered.is_complete(), policy == "include");
        let evidence = json!(rendered.inspect());
        if policy == "include" {
            let one = rendered.sites()[&key("one")].as_ref().unwrap().site();
            assert!(one.output().file(&"index.html".parse().unwrap()).is_some());
        } else {
            assert!(rendered.sites()[&key("one")].is_err());
            assert_eq!(evidence["sites"]["site:one"]["code"], "site_empty");
            assert_eq!(evidence["status"], "local_set_incomplete");
            assert_eq!(
                rendered.sites().len(),
                2,
                "never silently shrink the requested set"
            );
        }
        let two = rendered.sites()[&key("two")].as_ref().unwrap().site();
        assert!(two.output().file(&"index.html".parse().unwrap()).is_some());
        assert_eq!(evidence["build_selection"]["origin"], "workspace");
        assert_eq!(evidence["build_selection"]["local_drafts"], policy);
        if policy == "exclude" {
            assert_eq!(evidence["selection"]["content"]["content:one"], "exclude");
            assert_eq!(evidence["selection"]["content"]["content:two"], "include");
        } else {
            assert!(evidence["selection"].is_null());
        }
        let explicit = build::acquire_publications(
            &workspace,
            &[key("one")],
            Default::default(),
            Default::default(),
        )
        .expect("explicit invocation override");
        assert_eq!(
            explicit.build_selection().origin,
            gazette_sdk::workspace::BuildOrigin::Invocation
        );
        assert_eq!(explicit.build_selection().sites, [key("one")]);
    }
}

#[test]
fn configured_non_draft_build_renders_retained_pages_without_reintroducing_drafts() {
    let fixture = Fixture::with_pages(&["one", "two"], true, true);
    let workspace = format!(
        "[build]\nsites=['site:two','site:one']\nlocal_drafts='exclude'\n{}",
        fixture.workspace
    );
    let inputs =
        build::acquire_configured_publications(&workspace, Default::default(), Default::default())
            .expect("complete configured capture");
    let choices = recipe(&["two", "one"], None);
    let rendered = build::render_publications(
        &inputs,
        choices.options(Default::default()),
        Default::default(),
    )
    .expect("resolved local non-draft selection");
    assert!(rendered.is_complete());
    let one = rendered.sites()[&key("one")].as_ref().unwrap().site();
    assert!(
        one.output()
            .file(&"retained/index.html".parse().unwrap())
            .is_some()
    );
    assert!(one.output().file(&"index.html".parse().unwrap()).is_none());
    assert!(one.output().file(&"old.html".parse().unwrap()).is_none());
    let evidence = json!(rendered.inspect());
    assert_eq!(evidence["selection"]["content"]["content:one"], "exclude");
    assert_eq!(
        evidence["selection"]["content"]["content:one-retained"],
        "include"
    );
    assert_eq!(
        evidence["build_selection"]["sites"],
        json!(["site:two", "site:one"])
    );
}

#[test]
fn configured_build_defaults_refuse_missing_defaults_without_reading_source_fallbacks() {
    let fixture = Fixture::new(&["one"]);
    assert!(
        build::acquire_configured_publications(
            &fixture.workspace,
            Default::default(),
            Default::default()
        )
        .is_err()
    );
    let workspace = format!(
        "[build]\nsites=['site:one']\nlocal_drafts='include'\n{}",
        fixture.workspace
    );
    assert!(
        build::acquire_publications(&workspace, &[], Default::default(), Default::default())
            .is_err(),
        "explicit empty override must not become configured selection"
    );
}

#[test]
fn omitted_recipe_sites_reach_the_same_retained_render_with_workspace_order_and_policy() {
    use gazette_sdk::workspace::{BuildOrigin, LocalDrafts};
    for policy in ["include", "exclude"] {
        let fixture = Fixture::with_pages(&["one", "two"], true, true);
        let workspace = format!(
            "[build]\nsites=['site:two','site:one']\nlocal_drafts='{policy}'\n{}",
            fixture.workspace
        );
        let request = json!({"format":"gazette-local-publication-set/1",
            "workspace":{"kind":"supplied","source":workspace}});
        let unresolved =
            build::LocalPublicationRecipe::parse_json(request.to_string().as_bytes(), 1 << 20, 2)
                .expect("omitted sites remain unresolved configured intent");
        assert!(unresolved.invocation_sites().is_none());
        let resolved = unresolved
            .resolve(Default::default())
            .expect("one native configuration");
        assert_eq!(
            resolved.sites().cloned().collect::<Vec<_>>(),
            [key("two"), key("one")]
        );
        assert_eq!(resolved.build_selection().origin, BuildOrigin::Workspace);
        assert_eq!(
            resolved.build_selection().local_drafts,
            Some(if policy == "include" {
                LocalDrafts::Include
            } else {
                LocalDrafts::Exclude
            })
        );
        let inputs = build::acquire_configured_publications(
            &workspace,
            Default::default(),
            Default::default(),
        )
        .expect("existing native configured acquisition");
        let expected = build::render_publications(
            &inputs,
            resolved.options(Default::default()),
            Default::default(),
        )
        .expect("existing shared configured render");
        assert!(expected.is_complete());
        resolved
            .with_rendered(
                Default::default(),
                Default::default(),
                Default::default(),
                |actual| {
                    assert!(actual.is_complete());
                    assert_eq!(json!(actual.inspect()), json!(expected.inspect()));
                    assert_eq!(
                        json!(actual.inspect())["build_selection"]["sites"],
                        json!(["site:two", "site:one"])
                    );
                    for name in ["one", "two"] {
                        let actual = actual.sites()[&key(name)].as_ref().unwrap().site();
                        let expected = expected.sites()[&key(name)].as_ref().unwrap().site();
                        assert_eq!(actual.output().inventory(), expected.output().inventory());
                        for entry in expected.output().inventory().entries() {
                            assert_eq!(
                                actual.output().file(&entry.path),
                                expected.output().file(&entry.path)
                            );
                        }
                        assert!(actual.evidence().model_output.is_none());
                        assert!(
                            actual
                                .output()
                                .file(&"model.json".parse().unwrap())
                                .is_none()
                        );
                        assert!(
                            actual
                                .output()
                                .file(&"retained/index.html".parse().unwrap())
                                .is_some()
                        );
                        if name == "one" {
                            assert_eq!(
                                actual
                                    .output()
                                    .file(&"index.html".parse().unwrap())
                                    .is_some(),
                                policy == "include"
                            );
                        }
                    }
                },
            )
            .expect("actual resolved recipe acquisition and render");
        let mut explicit = request;
        explicit["sites"] = json!([{"site":"site:one"}]);
        let explicit =
            build::LocalPublicationRecipe::parse_json(explicit.to_string().as_bytes(), 1 << 20, 2)
                .unwrap()
                .resolve(Default::default())
                .unwrap();
        assert_eq!(explicit.build_selection().origin, BuildOrigin::Invocation);
        assert_eq!(explicit.sites().cloned().collect::<Vec<_>>(), [key("one")]);
        explicit
            .with_rendered(
                Default::default(),
                Default::default(),
                Default::default(),
                |actual| {
                    assert!(actual.is_complete());
                    assert_eq!(actual.sites().len(), 1);
                    assert_eq!(
                        json!(actual.inspect())["build_selection"]["origin"],
                        "invocation"
                    );
                    assert_eq!(
                        json!(actual.inspect())["build_selection"]["local_drafts"],
                        policy
                    );
                },
            )
            .expect("explicit override retains configured draft policy, not configured membership");
    }
}

#[test]
fn modular_workspace_builds_single_and_multi_site_sets_through_the_owned_recipe() {
    for names in [&["one"][..], &["one", "two"][..]] {
        let fixture = Fixture::new(names);
        let request = json!({
            "format":"gazette-local-publication-set/1",
            "workspace":{
                "kind":"local",
                "root":fixture.directory.path(),
                "workspace":"gazette.toml"
            },
            "sites":names.iter().map(|name| site(name)).collect::<Vec<_>>()
        });
        let recipe =
            build::LocalPublicationRecipe::parse_json(request.to_string().as_bytes(), 1 << 20, 2)
                .expect("strict local document-set recipe");
        let recipe = recipe
            .resolve(Default::default())
            .expect("retain configuration once");
        fs::write(
            fixture.directory.path().join("workspace.toml"),
            "not valid workspace TOML",
        )
        .expect("change only mutable configuration after resolution");
        recipe
            .with_rendered(
                Default::default(),
                Default::default(),
                Default::default(),
                |rendered| {
                    assert!(rendered.is_complete());
                    assert_eq!(rendered.sites().len(), names.len());
                    let inspection = serde_json::to_value(rendered.inspect()).expect("evidence");
                    assert_eq!(inspection["configuration"]["kind"], "documents");
                    assert_eq!(
                        inspection["configuration"]["documents"]
                            .as_array()
                            .expect("document evidence")
                            .len(),
                        2
                    );
                },
            )
            .expect("modular workspace build");
    }
}

fn site(name: &str) -> Value {
    json!({"site":format!("site:{name}"),"model_output":"model.json"})
}

#[test]
fn configured_local_addressing_reaches_the_shared_renderer_without_starting_listeners() {
    let fixture = Fixture::new(&["one", "two"]);
    let workspace = fixture
        .workspace
        .replace(
            "site='site:one'\npublic_base_url",
            "site='site:one'\nlocal_port=4311\npublic_base_url",
        )
        .replace(
            "site='site:two'\npublic_base_url",
            "site='site:two'\nlocal_port=4312\npublic_base_url",
        );
    for addressing in ["public", "local"] {
        let request = json!({"format":"gazette-local-publication-set/1", "workspace":{"kind":"supplied","source":workspace},"sites":[site("one"),site("two")],"addressing":addressing});
        let recipe =
            build::LocalPublicationRecipe::parse_json(request.to_string().as_bytes(), 1 << 20, 2)
                .unwrap();
        let recipe = recipe
            .resolve(Default::default())
            .expect("fixed supplied configuration");
        recipe
            .with_rendered(
                Default::default(),
                Default::default(),
                Default::default(),
                |rendered| {
                    assert!(rendered.is_complete());
                    assert_eq!(rendered.preview_urls().is_some(), addressing == "local");
                    for (name, port) in [("one", 4311), ("two", 4312)] {
                        let rendered = rendered.sites().get(&key(name)).unwrap().as_ref().unwrap();
                        let bytes = rendered
                            .site()
                            .output()
                            .file(&"model.json".parse().unwrap())
                            .unwrap();
                        let model: Value = serde_json::from_slice(bytes).unwrap();
                        let expected = if addressing == "local" {
                            format!("http://127.0.0.1:{port}/mount/")
                        } else {
                            format!("https://{name}.example/mount/")
                        };
                        assert!(model.to_string().contains(&expected), "{model}");
                    }
                },
            )
            .unwrap();
    }
    let missing = json!({"format":"gazette-local-publication-set/1", "workspace":{"kind":"supplied","source":fixture.workspace},"sites":[site("one")],"addressing":"local"});
    let recipe =
        build::LocalPublicationRecipe::parse_json(missing.to_string().as_bytes(), 1 << 20, 2)
            .unwrap();
    let recipe = recipe
        .resolve(Default::default())
        .expect("valid workspace before local address refusal");
    let error = recipe
        .with_rendered(
            Default::default(),
            Default::default(),
            Default::default(),
            |_| panic!("no public fallback"),
        )
        .unwrap_err();
    assert_eq!(error.code(), "publication_set_selection");
}

fn recipe(names: &[&str], selection: Option<authored::Selection>) -> build::PublicationRecipe {
    build::PublicationRecipe::new(
        names
            .iter()
            .map(|name| serde_json::from_value(site(name)).expect("native recipe paths"))
            .collect(),
        selection,
        None,
        128,
    )
    .expect("distinct explicit set")
}

fn key(name: &str) -> Key {
    format!("site:{name}").parse().expect("site identity")
}

#[test]
fn configured_output_coordinates_use_retained_workspace_without_writing_or_cwd_fallback() {
    let fixture = Fixture::new(&["one"]);
    let config = fixture.workspace.replace(
        "site='site:one'\npublic_base_url",
        "site='site:one'\noutput={directory='generated/site'}\npublic_base_url",
    );
    fs::write(fixture.directory.path().join("workspace.toml"), &config).unwrap();
    let loaded = gazette_application::configuration::load(
        &gazette_adapter_fs::FileSystem,
        fixture.directory.path(),
        &"gazette.toml".parse().unwrap(),
        Default::default(),
    )
    .unwrap();
    let inputs = gazette_application::build::acquire_resolved_publications(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        loaded.into_resolved(),
        &[key("one")],
        Default::default(),
    )
    .unwrap();
    let expected = fixture.directory.path().join("generated/site");
    assert_eq!(
        inputs.sites()[&key("one")]
            .as_ref()
            .unwrap()
            .output_directory()
            .unwrap(),
        Some(expected.clone())
    );
    assert!(
        !expected.exists(),
        "coordinates neither create nor grant output ownership"
    );
    let detached = build::acquire_publications(
        &config,
        &[key("one")],
        Default::default(),
        Default::default(),
    )
    .unwrap();
    assert!(
        detached.sites()[&key("one")]
            .as_ref()
            .unwrap()
            .output_directory()
            .is_err()
    );
    for directory in [None, Some(expected.to_str().unwrap())] {
        let config = directory.map_or_else(
            || fixture.workspace.clone(),
            |directory| {
                fixture.workspace.replace(
                    "site='site:one'\npublic_base_url",
                    &format!(
                        "site='site:one'\noutput={{directory={directory:?}}}\npublic_base_url"
                    ),
                )
            },
        );
        let inputs = build::acquire_publications(
            &config,
            &[key("one")],
            Default::default(),
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            inputs.sites()[&key("one")]
                .as_ref()
                .unwrap()
                .output_directory()
                .unwrap(),
            directory.map(std::path::PathBuf::from)
        );
    }
    assert!(!expected.exists());
}

#[test]
fn math_recipe_defaults_keep_execution_denied_and_validate_explicit_bounds() {
    let base = json!({"format":"gazette-local-publication-set/1", "workspace":{"kind":"supplied","source":"not opened during parsing"}, "sites":[site("one")]});
    let parse = |value: &Value| {
        build::LocalPublicationRecipe::parse_json(value.to_string().as_bytes(), 65536, 2)
    };
    let schema = build::LocalPublicationRecipe::input_schema();
    let validator = jsonschema::validator_for(schema.as_value()).expect("native recipe schema");
    assert!(
        parse(&base)
            .expect("omitted selection")
            .math_generation()
            .is_none()
    );
    let defaults = json!(build::MathGeneration::default());
    assert_eq!(defaults["allow_execute"], false);
    for permission in [false, true] {
        let mut value = base.clone();
        value["math_generation"] = defaults.clone();
        value["math_generation"]["allow_execute"] = json!(permission);
        assert!(validator.is_valid(&value));
        let parsed = parse(&value).expect("explicit complete object");
        let options = parsed.math_generation().expect("selected effect options");
        assert_eq!(options.execution.allow_execute, permission);
        assert_eq!(options.execution.timeout.as_millis(), 30_000);
        assert_eq!(options.formulas, 4096);
    }
    let mut missing_permission = defaults.clone();
    missing_permission
        .as_object_mut()
        .expect("options object")
        .remove("allow_execute");
    for (selected, formulas) in [
        (json!({}), 4096),
        (missing_permission, 4096),
        (json!({"formulas": 2}), 2),
    ] {
        let mut value = base.clone();
        value["math_generation"] = selected.clone();
        assert!(validator.is_valid(&value));
        let parsed = parse(&value).expect("documented optional math overrides");
        let options = parsed.math_generation().expect("explicit math selection");
        assert!(!options.execution.allow_execute);
        assert_eq!(options.execution.max_input_bytes, 2 << 20);
        assert_eq!(options.execution.timeout.as_millis(), 30_000);
        assert_eq!(options.output_bytes, 64 << 20);
        assert_eq!(options.formulas, formulas);
    }
    let mut invalid = vec![
        Value::Null,
        json!([]),
        json!([false, 16_777_216, 30_000, 4096, 67_108_864]),
    ];
    for (field, bad) in [
        ("max_input_bytes", json!(0)),
        ("max_input_bytes", json!(67_108_865)),
        ("timeout_ms", json!(0)),
        ("timeout_ms", json!(300_001)),
        ("formulas", json!(0)),
        ("formulas", json!(4097)),
        ("output_bytes", json!(0)),
        ("output_bytes", json!(67_108_865)),
        ("allow_execute", json!("true")),
        ("unknown", json!(true)),
    ] {
        let mut options = defaults.clone();
        options[field] = bad;
        invalid.push(options);
    }
    for options in invalid {
        let mut value = base.clone();
        value["math_generation"] = options;
        assert!(!validator.is_valid(&value), "{value}");
        assert!(parse(&value).is_err(), "{value}");
    }
    let mut duplicate = base;
    duplicate["math_generation"] = defaults;
    let duplicate = duplicate.to_string().replace(
        "\"allow_execute\":false",
        "\"allow_execute\":true,\"allow_execute\":false",
    );
    assert!(build::LocalPublicationRecipe::parse_json(duplicate.as_bytes(), 65536, 2).is_err());
}

#[test]
fn diagram_recipe_schema_matches_explicit_permission_and_bounded_named_options() {
    let base = json!({"format":"gazette-local-publication-set/1", "workspace":{"kind":"supplied","source":"not opened during parsing"}, "sites":[site("one")]});
    let parse = |value: &Value| {
        build::LocalPublicationRecipe::parse_json(value.to_string().as_bytes(), 65536, 2)
    };
    let schema = build::LocalPublicationRecipe::input_schema();
    let validator = jsonschema::validator_for(schema.as_value()).expect("native recipe schema");
    let preparation_schema = build::LocalPreparationRecipe::input_schema();
    assert_eq!(
        schema.as_value()["properties"]["diagram_generation"],
        preparation_schema.as_value()["properties"]["diagram_generation"]
    );
    assert!(
        parse(&base)
            .expect("omitted selection")
            .diagram_generation()
            .is_none()
    );
    let defaults = json!(build::DiagramGeneration::default());
    assert_eq!(defaults["allow_execute"], false);
    for permission in [false, true] {
        let mut value = base.clone();
        value["diagram_generation"] = defaults.clone();
        value["diagram_generation"]["allow_execute"] = json!(permission);
        assert!(validator.is_valid(&value));
        let options = parse(&value)
            .expect("named bounded options")
            .diagram_generation()
            .expect("selected");
        assert_eq!(options.execution.allow_execute, permission);
        assert_eq!(options.diagrams, 256);
    }
    let mut invalid = vec![Value::Null, json!([]), json!({})];
    for (field, bad) in [
        ("diagrams", json!(0)),
        ("diagrams", json!(257)),
        ("max_input_bytes", json!(0)),
        ("timeout_ms", json!(300001)),
        ("output_bytes", json!(67108865)),
        ("diagnostic_bytes", json!(65537)),
        ("allow_execute", json!("true")),
        ("unknown", json!(true)),
    ] {
        let mut value = defaults.clone();
        value[field] = bad;
        invalid.push(value);
    }
    for options in invalid {
        let mut value = base.clone();
        value["diagram_generation"] = options;
        assert!(!validator.is_valid(&value), "{value}");
        assert!(parse(&value).is_err(), "{value}");
    }
}

#[test]
fn single_and_multi_site_recipes_match_existing_borrowed_rendering_on_retained_bytes() {
    for names in [&["one"][..], &["one", "two"][..]] {
        let fixture = Fixture::new(names);
        let recipe = recipe(names, None);
        let keys = recipe.sites().cloned().collect::<Vec<_>>();
        let inputs = build::acquire_publications(
            &fixture.workspace,
            &keys,
            Default::default(),
            Default::default(),
        )
        .expect("actual pinned capture");
        let model = "model.json".parse().expect("model path");
        let expected = build::render_publications(
            &inputs,
            build::PublicationSetRenderOptions {
                derive_selection: false,
                preview_link_bases: None,
                sites: keys
                    .iter()
                    .map(|key| {
                        (
                            key.clone(),
                            build::SiteRenderOptions {
                                render_year: None,
                                model_output: Some(&model),
                                graph: None,
                                limits: Default::default(),
                            },
                        )
                    })
                    .collect(),
                selection: None,
                review_responses: None,
                preview_urls: None,
            },
            Default::default(),
        )
        .expect("existing production API");
        assert!(expected.is_complete());
        fs::write(
            fixture.directory.path().join("theme/templates/page.html"),
            "changed after capture",
        )
        .expect("disposable source edit");
        let actual = build::render_publications(
            &inputs,
            recipe.options(Default::default()),
            Default::default(),
        )
        .expect("shared recipe production projection");
        assert!(actual.is_complete());
        assert_eq!(json!(actual.inspect()), json!(expected.inspect()));
        for name in names {
            let actual = actual.sites()[&key(name)]
                .as_ref()
                .expect("site complete")
                .site()
                .output();
            let expected = expected.sites()[&key(name)]
                .as_ref()
                .expect("site complete")
                .site()
                .output();
            assert_eq!(
                actual.inventory().identity(),
                expected.inventory().identity()
            );
            for entry in expected.inventory().entries() {
                if matches!(entry.content, source::Content::File { .. }) {
                    assert_eq!(actual.file(&entry.path), expected.file(&entry.path));
                }
            }
            let html = actual
                .file(&"index.html".parse().expect("HTML path"))
                .expect("real HTML");
            assert!(String::from_utf8_lossy(html).contains(&format!("Retained {name}")));
            let model: Value =
                serde_json::from_slice(actual.file(&model).expect("real machine model"))
                    .expect("generated JSON");
            assert!(model.is_object());
            assert!(
                actual
                    .file(&"old.html".parse().expect("alias path"))
                    .is_some()
            );
        }
        assert_eq!(
            fs::read_to_string(fixture.directory.path().join("theme/templates/page.html"))
                .expect("source readback"),
            "changed after capture"
        );
        assert!(
            !fixture.directory.path().join("index.html").exists(),
            "rendering never stages output"
        );
    }
}

#[test]
fn cache_keys_are_application_derived_and_change_with_retained_configuration() {
    let fixture = Fixture::new(&["one"]);
    let site = key("one");
    let plan = |workspace: &str| {
        let inputs = build::acquire_publications(
            workspace,
            std::slice::from_ref(&site),
            Default::default(),
            Default::default(),
        )
        .expect("exact retained acquisition");
        let recipe = recipe(&["one"], None);
        let options = recipe.options(Default::default());
        let plan = build::plan_build_cache(&inputs, &options, Default::default())
            .expect("application-owned pre-render evidence closure");
        let key = plan.key(&site).expect("site key").clone();
        let rendered = build::render_publications(&inputs, options, Default::default())
            .expect("exact successful render");
        assert!(rendered.is_complete());
        key
    };
    let first = plan(&fixture.workspace);
    let changed = fixture
        .workspace
        .replace("Recipe fixture", "Changed retained configuration");
    let second = plan(&changed);
    assert_ne!(
        first, second,
        "a caller cannot preserve a warm identity across changed retained configuration"
    );
    assert_eq!(first.site(), &site);
    assert_eq!(second.site(), &site);
}

#[test]
fn optional_model_output_changes_only_the_requested_debug_artifact_and_cache_identity() {
    let fixture = Fixture::new(&["one"]);
    let site_key = key("one");
    let inputs = build::acquire_publications(
        &fixture.workspace,
        std::slice::from_ref(&site_key),
        Default::default(),
        Default::default(),
    )
    .expect("one exact retained acquisition");
    let explicit = recipe(&["one"], None);
    let omitted = build::PublicationRecipe::new(
        vec![serde_json::from_value(json!({"site":"site:one"})).expect("omitted debug artifact")],
        None,
        None,
        1,
    )
    .expect("same explicit site selection");
    let explicit_options = explicit.options(Default::default());
    let omitted_options = omitted.options(Default::default());
    assert!(omitted_options.sites[&site_key].model_output.is_none());
    let explicit_plan = build::plan_build_cache(&inputs, &explicit_options, Default::default())
        .expect("explicit artifact cache closure");
    let omitted_plan = build::plan_build_cache(&inputs, &omitted_options, Default::default())
        .expect("omitted artifact cache closure");
    assert_ne!(explicit_plan.key(&site_key), omitted_plan.key(&site_key));
    let with_model = build::render_publications(&inputs, explicit_options, Default::default())
        .expect("explicit model render");
    let without_model = build::render_publications(&inputs, omitted_options, Default::default())
        .expect("render without a serialized debug prerequisite");
    assert!(with_model.is_complete() && without_model.is_complete());
    let with_model = with_model.sites()[&site_key].as_ref().unwrap().site();
    let without_model = without_model.sites()[&site_key].as_ref().unwrap().site();
    let model_path: source::EntryPath = "model.json".parse().unwrap();
    assert_eq!(
        with_model.evidence().model_output.as_ref(),
        Some(&model_path)
    );
    assert!(without_model.evidence().model_output.is_none());
    assert!(
        json!(without_model.evidence())
            .get("model_output")
            .is_none()
    );
    assert!(with_model.output().file(&model_path).is_some());
    assert!(without_model.output().file(&model_path).is_none());
    let mut formats = with_model.media_types().clone();
    assert!(formats.remove(&model_path).is_some());
    assert_eq!(&formats, without_model.media_types());
    assert_eq!(
        without_model.output().inventory().entries().len() + 1,
        with_model.output().inventory().entries().len()
    );
    for entry in with_model.output().inventory().entries() {
        if entry.path != model_path {
            assert_eq!(
                without_model.output().file(&entry.path),
                with_model.output().file(&entry.path),
                "unchanged output {}",
                entry.path
            );
        }
    }
    assert_eq!(
        json!(with_model.evidence().pages),
        json!(without_model.evidence().pages)
    );
}

#[test]
fn local_recipe_schema_and_real_render_accept_an_omitted_model_output_without_a_default() {
    let fixture = Fixture::new(&["one"]);
    let request = json!({
        "format":"gazette-local-publication-set/1",
        "workspace":{"kind":"supplied","source":fixture.workspace},
        "sites":[{"site":"site:one"}]
    });
    let schema = serde_json::to_value(build::LocalPublicationRecipe::input_schema()).unwrap();
    let validator = jsonschema::validator_for(&schema).expect("native request schema");
    validator
        .validate(&request)
        .expect("model_output is not required");
    let recipe =
        build::LocalPublicationRecipe::parse_json(request.to_string().as_bytes(), 1 << 20, 1)
            .expect("actual native omitted-model request");
    let recipe = recipe
        .resolve(Default::default())
        .expect("native supplied workspace resolution");
    assert!(
        recipe.options(Default::default()).sites[&key("one")]
            .model_output
            .is_none()
    );
    recipe
        .with_rendered(
            Default::default(),
            Default::default(),
            Default::default(),
            |rendered| {
                assert!(rendered.is_complete());
                let site = rendered.sites()[&key("one")].as_ref().unwrap().site();
                assert!(site.output().file(&"index.html".parse().unwrap()).is_some());
                assert!(site.output().file(&"old.html".parse().unwrap()).is_some());
                assert!(site.output().file(&"model.json".parse().unwrap()).is_none());
                assert!(site.evidence().model_output.is_none());
                assert!(!site.media_types().contains_key("model.json"));
            },
        )
        .expect("same retained compiler and Theme without debug serialization");
    let mut invalid = request;
    invalid["sites"][0]["model_output"] = json!("../escape");
    assert!(!validator.is_valid(&invalid));
    assert!(
        build::LocalPublicationRecipe::parse_json(invalid.to_string().as_bytes(), 1 << 20, 1)
            .is_err()
    );
}

#[test]
fn candidate_and_incomplete_site_evidence_survive_the_shared_recipe() {
    let fixture = Fixture::new(&["one", "two"]);
    let intent: authored::Selection = serde_json::from_value(
        json!({"content":{"content:one":"include","content:two":"include"},"assets":{}}),
    )
    .expect("native candidate input");
    let mut choices: Vec<build::SiteRecipe> = [site("one"), site("two")]
        .into_iter()
        .map(|value| serde_json::from_value(value).expect("site recipe"))
        .collect();
    choices[1].model_output = Some("index.html".parse().expect("valid colliding output path"));
    let recipe = build::PublicationRecipe::new(choices, Some(intent.clone()), None, 2)
        .expect("exact candidate set");
    let inputs = build::acquire_publications(
        &fixture.workspace,
        &recipe.sites().cloned().collect::<Vec<_>>(),
        Default::default(),
        Default::default(),
    )
    .expect("real source capture");
    let result = build::render_publications(
        &inputs,
        recipe.options(Default::default()),
        Default::default(),
    )
    .expect("site-local failure remains an outcome");
    assert!(!result.is_complete());
    assert!(result.sites()[&key("one")].is_ok());
    assert!(result.sites()[&key("two")].is_err());
    assert_eq!(json!(result.selection()), json!(Some(intent)));
    assert_eq!(result.sites().len(), 2);
    assert_eq!(json!(result.inspect())["status"], "local_set_incomplete");
    let bounded = build::render_publications(
        &inputs,
        recipe.options(source::Limits {
            total_bytes: 1,
            ..Default::default()
        }),
        Default::default(),
    )
    .expect("bounded set report");
    assert!(!bounded.is_complete());
    assert!(bounded.sites().values().all(Result::is_err));
    assert_eq!(bounded.sites().len(), 2);
}

#[test]
fn native_recipe_paths_keys_and_candidate_maps_reject_invalid_values() {
    for field in ["model_output"] {
        for invalid in [
            "",
            "/absolute",
            "../escape",
            "a//b",
            "a/./b",
            "a\\b",
            "a\u{0}b",
        ] {
            let mut value = site("one");
            value[field] = json!(invalid);
            assert!(
                serde_json::from_value::<build::SiteRecipe>(value).is_err(),
                "{field}: {invalid:?}"
            );
        }
    }
    for invalid in [
        json!({"site":"missing-namespace","model_output":"model.json"}),
        json!({"site":"site:one","model_output":"model.json","unknown":true}),
        json!({"site":"site:one","model_output":"model.json","graph":{"template":"page.html"}}),
    ] {
        assert!(serde_json::from_value::<build::SiteRecipe>(invalid).is_err());
    }
    for invalid in [
        json!({"content":{},"assets":{},"unknown":true}),
        json!({"content":{}}),
        json!({"content":{"content:one":{"include":null}},"assets":{}}),
        json!({"content":{"content:one":"Include"},"assets":{}}),
        json!({"content":[],"assets":{}}),
        json!({"content":{},"assets":{"../asset":"include"}}),
    ] {
        assert!(serde_json::from_value::<authored::Selection>(invalid).is_err());
    }
    for (names, maximum) in [
        (&[][..], 128),
        (&["one", "one"][..], 2),
        (&["one", "two"][..], 1),
    ] {
        let sites = names
            .iter()
            .map(|name| serde_json::from_value(site(name)).expect("valid individual recipe"))
            .collect();
        assert!(build::PublicationRecipe::new(sites, None, None, maximum).is_err());
    }
    let duplicate = br#"{"content":{"content:one":"include","content:one":"exclude"},"assets":{}}"#;
    assert!(
        gazette_sdk::json::parse(duplicate).is_err(),
        "the shared wire decoder retains duplicate-key evidence before typed maps"
    );
}

#[test]
fn candidate_and_acquisition_schemas_match_native_values_and_integer_ranges() {
    let schema = schemars::schema_for!(authored::Selection);
    let validator = jsonschema::validator_for(schema.as_value()).expect("native selection schema");
    for (value, valid) in [
        (
            json!({"content":{"content:one":"include"},"assets":{"one/file.txt":"exclude"}}),
            true,
        ),
        (
            json!({"content":{"content:one":"Include"},"assets":{}}),
            false,
        ),
        (
            json!({"content":{"content:one":{"include":null}},"assets":{}}),
            false,
        ),
        (json!({"content":{}}), false),
    ] {
        assert_eq!(validator.is_valid(&value), valid);
        assert_eq!(
            serde_json::from_value::<authored::Selection>(value).is_ok(),
            valid
        );
    }
    let schema = schemars::schema_for!(build::PublicationSetAcquisitionLimits);
    let validator =
        jsonschema::validator_for(schema.as_value()).expect("native acquisition schema");
    for value in [
        json!({}),
        json!({"sites":usize::MAX}),
        json!({"sources":{"sources":4,"entries":64,"bytes":u64::MAX,"per_source":{}}}),
    ] {
        assert!(validator.is_valid(&value));
        assert!(serde_json::from_value::<build::PublicationSetAcquisitionLimits>(value).is_ok());
    }
    let beyond: Value = serde_json::from_str(&((usize::MAX as u128) + 1).to_string())
        .expect("oversized JSON integer");
    let value = json!({"sites":beyond});
    assert!(!validator.is_valid(&value));
    assert!(serde_json::from_value::<build::PublicationSetAcquisitionLimits>(value).is_err());
}

#[test]
fn entry_path_schema_matches_native_literal_component_grammar() {
    let schema = schemars::schema_for!(source::EntryPath);
    let validator = jsonschema::validator_for(schema.as_value()).expect("native path schema");
    for character in (0..=0x9f).map(|value| char::from_u32(value).expect("Unicode scalar")) {
        let value = json!(format!("before{character}after"));
        let native = serde_json::from_value::<source::EntryPath>(value.clone()).is_ok();
        assert_eq!(
            validator.is_valid(&value),
            native,
            "U+{:04X}",
            character as u32
        );
    }
    for path in ["", ".", "..", "a/.", "a/..", "/a", "a/", "a//b", "a\\b"] {
        let value = json!(path);
        assert!(!validator.is_valid(&value), "{path:?}");
        assert!(serde_json::from_value::<source::EntryPath>(value).is_err());
    }
    for path in [
        "literal%2Fcolon:name",
        "日本語/é.md",
        "line\u{2028}separator",
        "line\u{2029}separator",
        ".\u{2028}",
        "..\u{2029}",
    ] {
        let value = json!(path);
        assert!(validator.is_valid(&value), "{path:?}");
        assert!(serde_json::from_value::<source::EntryPath>(value).is_ok());
    }
}

#[test]
fn render_year_is_explicit_bounded_and_boundary_default_never_overrides_it() {
    let fixture = Fixture::new(&["one"]);
    let base = json!({"format":"gazette-local-publication-set/1", "workspace":{"kind":"supplied","source":fixture.workspace}, "sites":[{"site":"site:one"}]});
    for year in [None, Some(2040)] {
        let mut input = base.clone();
        if let Some(year) = year {
            input["render_year"] = json!(year);
        }
        let recipe = build::LocalPublicationRecipe::parse_json(
            &serde_json::to_vec(&input).unwrap(),
            1 << 20,
            4,
        )
        .unwrap();
        let defaulted = recipe.with_default_render_year(authored::RenderYear::new(2050).unwrap());
        let resolved = defaulted.resolve(Default::default()).unwrap();
        assert_eq!(
            resolved.options(Default::default()).sites[&key("one")]
                .render_year
                .unwrap()
                .get(),
            year.unwrap_or(2050)
        );
    }
    let recipe =
        build::LocalPublicationRecipe::parse_json(&serde_json::to_vec(&base).unwrap(), 1 << 20, 4)
            .unwrap()
            .resolve(Default::default())
            .unwrap();
    assert!(
        recipe.options(Default::default()).sites[&key("one")]
            .render_year
            .is_none(),
        "SDK omission has no ambient clock fallback"
    );
    for bad in [
        json!(0),
        json!(10000),
        json!(-1),
        json!(2040.5),
        json!("2040"),
        json!(null),
    ] {
        let mut input = base.clone();
        input["render_year"] = bad;
        assert!(
            build::LocalPublicationRecipe::parse_json(
                &serde_json::to_vec(&input).unwrap(),
                1 << 20,
                4
            )
            .is_err()
        );
    }
}
