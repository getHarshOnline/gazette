//! The target's public boundary preserves complete native artifacts and routes.

use gazette_adapter_mintlify::Mintlify;
use gazette_core::documentation::{
    Error, Format, ProjectItem, ProjectModule, Reference, Request, Target,
};
use gazette_core::source::{Content, Limits};
use gazette_ports::documentation::DocumentationTarget;
use serde_json::{Value, json};

mod support;
use support::Input;

struct Config<'a> {
    name: &'a str,
    theme: &'a str,
    primary_color: &'a str,
}

fn assemble(
    config: Config<'_>,
    projects: &[ProjectModule<'_>],
    limits: Limits,
) -> Result<gazette_core::source::CapturedSource, Error> {
    let target = Target::Mintlify {
        theme: config.theme.into(),
        primary_color: config.primary_color.into(),
        light_color: None,
        dark_color: None,
        telemetry: None,
    };
    Mintlify::new(&gazette_adapter_markdown::Comrak).assemble(
        Request {
            name: config.name,
            public_base_url: None,
            target: &target,
            projects,
            usage: None,
        },
        limits,
    )
}

fn config() -> Config<'static> {
    Config {
        name: "Reference publication",
        theme: "mint",
        primary_color: "#123abc",
    }
}

fn fixture() -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({
        "schemaVersion": "2.0", "variant": "project", "id": 0, "kind": 1,
        "name": "Example", "flags": {}, "files": {}, "packageVersion": "1.2.3",
        "children": [{"variant":"declaration", "id":1, "kind":64, "name":"run", "flags":{},
            "comment":{"summary":[{"kind":"text","text":"Preserved docs and example."}]},
            "signatures":[{"variant":"signature", "id":2, "kind":4096, "name":"run", "flags":{},
                "type":{"type":"reference","name":"Result","target":3}}]},
            {"variant":"declaration", "id":3, "kind":2097152, "name":"Result", "flags":{},
             "type":{"type":"intrinsic","name":"string"}}]
    }))
    .expect("fixture")
}

fn project<'a>(slug: &'a str, input: &'a Input) -> ProjectModule<'a> {
    ProjectModule {
        slug,
        title: slug,
        manual: None,
        navigation: vec![ProjectItem::Reference(Reference {
            pages: None,
            slug: "library",
            title: "Library reference",
            artifact: input.artifact(Format::TypeDocJson),
        })],
    }
}

#[test]
fn configured_public_canonical_base_is_optional_and_never_replaced_by_preview_links() {
    let input = Input::new(&fixture());
    let projects = [project("library", &input)];
    let target = Target::Mintlify {
        theme: "mint".into(),
        primary_color: "#123abc".into(),
        light_color: None,
        dark_color: None,
        telemetry: None,
    };
    let preview = serde_json::from_value(json!({
        "https://docs.example.test/reference/": "http://127.0.0.1:43123/reference/"
    }))
    .unwrap();
    for (base, canonical) in [
        (None, None),
        (
            Some("https://docs.example.test/".parse().unwrap()),
            Some("https://docs.example.test"),
        ),
        (
            Some("https://docs.example.test/reference/".parse().unwrap()),
            Some("https://docs.example.test/reference"),
        ),
    ] {
        for local in [false, true] {
            let output = Mintlify::new(&gazette_adapter_markdown::Comrak)
                .with_preview_link_bases(local.then_some(&preview))
                .assemble(
                    Request {
                        name: "References",
                        public_base_url: base.as_ref(),
                        target: &target,
                        projects: &projects,
                        usage: None,
                    },
                    Limits::default(),
                )
                .unwrap();
            let config: Value =
                serde_json::from_slice(output.file(&"docs.json".parse().unwrap()).unwrap())
                    .unwrap();
            if let Some(canonical) = canonical {
                assert_eq!(config["seo"], json!({"metatags":{"canonical":canonical}}));
            } else {
                assert!(
                    config.get("seo").is_none(),
                    "an absent public owner must not be inferred from preview bindings"
                );
            }
        }
    }
}

#[test]
fn publication_reservation_reaches_the_docs_host_and_telemetry_is_explicit() {
    use gazette_core::publication::usage::{CrawlerGroup, UsagePolicy};
    let input = Input::new(&fixture());
    let projects = [project("library", &input)];
    let base = "https://docs.example.test/".parse().unwrap();
    let everything = ["/".to_owned()];
    let reserved = UsagePolicy {
        terms_url: "https://site.example.test/terms/".into(),
        crawlers: vec![
            CrawlerGroup {
                user_agent: "*",
                allow: &everything,
                disallow: &[],
            },
            CrawlerGroup {
                user_agent: "TrainingBot",
                allow: &[],
                disallow: &everything,
            },
        ],
    };
    let rules_only = UsagePolicy {
        crawlers: Vec::new(),
        ..reserved.clone()
    };
    let file = |output: &gazette_core::source::CapturedSource, path: &str| {
        output
            .file(&path.parse().unwrap())
            .map(|bytes| String::from_utf8(bytes.to_vec()).unwrap())
    };
    for (telemetry, usage) in [
        (Some(false), Some(&reserved)),
        (Some(true), Some(&rules_only)),
        (None, None),
    ] {
        let mut configured =
            json!({"kind": "mintlify", "theme": "mint", "primary_color": "#123abc"});
        if let Some(enabled) = telemetry {
            configured["telemetry"] = enabled.into();
        }
        let target: Target = serde_json::from_value(configured.clone()).unwrap();
        assert_eq!(serde_json::to_value(&target).unwrap(), configured);
        let output = Mintlify::new(&gazette_adapter_markdown::Comrak)
            .assemble(
                Request {
                    name: "References",
                    public_base_url: Some(&base),
                    target: &target,
                    projects: &projects,
                    usage,
                },
                Limits::default(),
            )
            .unwrap();
        let docs = file(&output, "docs.json").unwrap();
        let config: Value = serde_json::from_str(&docs).unwrap();
        match telemetry {
            Some(enabled) => assert_eq!(
                config["integrations"],
                json!({"telemetry": {"enabled": enabled}})
            ),
            None => assert!(config.get("integrations").is_none()),
        }
        let ignores = file(&output, ".mintignore").unwrap();
        let Some(usage) = usage else {
            assert_eq!(
                config["seo"],
                json!({"metatags": {"canonical": "https://docs.example.test"}})
            );
            assert!(file(&output, "robots.txt").is_none());
            assert!(file(&output, ".well-known/tdmrep.json").is_none());
            assert!(!ignores.contains(".well-known"));
            continue;
        };
        assert_eq!(
            config["seo"],
            json!({"metatags": {
                "canonical": "https://docs.example.test",
                "tdm-reservation": "1",
                "tdm-policy": "https://site.example.test/terms/"
            }})
        );
        assert!(docs.find("\"canonical\"") < docs.find("\"tdm-reservation\""));
        assert_eq!(
            file(&output, ".well-known/tdmrep.json").unwrap(),
            "[{\"location\":\"/*\",\"tdm-reservation\":1,\"tdm-policy\":\"https://site.example.test/terms/\"}]\n"
        );
        assert!(ignores.contains("\n!.well-known/\n!.well-known/tdmrep.json\n"));
        if usage.crawlers.is_empty() {
            assert!(file(&output, "robots.txt").is_none());
        } else {
            assert_eq!(
                file(&output, "robots.txt").unwrap(),
                "# Terms of use: https://site.example.test/terms/\n\nUser-agent: *\nAllow: /\n\nUser-agent: TrainingBot\nDisallow: /\n\nSitemap: https://docs.example.test/sitemap.xml\n"
            );
        }
    }
}

#[test]
fn explicit_mode_colors_survive_target_assembly_and_reject_invalid_values() {
    let input = Input::new(&fixture());
    let projects = [project("library", &input)];
    for light in [
        None,
        Some("#abc"),
        Some("#a4c9e7"),
        Some("green"),
        Some("#12345g"),
    ] {
        for dark in [None, Some("#305c85"), Some("")] {
            let mut configured = json!({
                "kind": "mintlify", "theme": "mint", "primary_color": "#123abc"
            });
            if let Some(color) = light {
                configured["light_color"] = color.into();
            }
            if let Some(color) = dark {
                configured["dark_color"] = color.into();
            }
            let target: Target = serde_json::from_value(configured.clone()).unwrap();
            assert_eq!(serde_json::to_value(&target).unwrap(), configured);
            let result = Mintlify::new(&gazette_adapter_markdown::Comrak).assemble(
                Request {
                    name: "Reference",
                    public_base_url: None,
                    target: &target,
                    projects: &projects,
                    usage: None,
                },
                Limits::default(),
            );
            if matches!(light, Some("green" | "#12345g")) || dark == Some("") {
                assert!(
                    result.is_err(),
                    "invalid mode color must refuse the complete target"
                );
                continue;
            }
            let output = result.unwrap();
            let config: Value =
                serde_json::from_slice(output.file(&"docs.json".parse().unwrap()).unwrap())
                    .unwrap();
            let mut colors = json!({"primary": "#123abc"});
            if let Some(color) = light {
                colors["light"] = color.into();
            }
            if let Some(color) = dark {
                colors["dark"] = color.into();
            }
            assert_eq!(config["colors"], colors);
        }
    }
}

#[test]
fn multiple_projects_assemble_exact_artifacts_and_distinct_navigation() {
    let bytes = fixture();
    let input = Input::new(&bytes);
    let projects = [project("pale-node", &input), project("gazette-js", &input)];
    let first = assemble(config(), &projects, Limits::default()).expect("complete target");
    let second = assemble(config(), &projects, Limits::default()).expect("repeat target");
    assert_eq!(first.inventory(), second.inventory());
    for entry in first.inventory().entries() {
        if matches!(entry.content, Content::File { .. }) {
            assert_eq!(first.file(&entry.path), second.file(&entry.path));
        }
    }
    let docs: Value = serde_json::from_slice(
        first
            .file(&"docs.json".parse().expect("path"))
            .expect("configuration"),
    )
    .expect("JSON");
    let groups = docs["navigation"]["dropdowns"]
        .as_array()
        .expect("project selector");
    assert_eq!(groups.len(), 2);
    for (group, project) in groups.iter().zip(&projects) {
        assert_eq!(group["dropdown"], project.title);
        let sdk = &group["pages"][0]["sdk"];
        assert_eq!(sdk["format"], "typedoc");
        assert_eq!(sdk["directory"], format!("{}/library", project.slug));
        let path = sdk["source"]
            .as_str()
            .expect("local source")
            .parse()
            .expect("path");
        assert_eq!(first.file(&path), Some(bytes.as_slice()));
    }
    assert_eq!(
        first.file(&".mintignore".parse().expect("path")),
        Some(b"artifacts/**\n".as_slice())
    );
}

#[test]
fn one_project_can_compose_multiple_native_reference_sources() {
    let bytes = fixture();
    let input = Input::new(&bytes);
    let mut module = project("client", &input);
    module.navigation.push(ProjectItem::Reference(Reference {
        pages: None,
        slug: "server",
        title: "Server package",
        artifact: input.artifact(Format::TypeDocJson),
    }));
    let tree = assemble(config(), &[module], Limits::default()).expect("composed references");
    assert!(
        tree.file(&"artifacts/client/library.json".parse().expect("path"))
            .is_some()
    );
    assert!(
        tree.file(&"artifacts/client/server.json".parse().expect("path"))
            .is_some()
    );
}

#[test]
fn malformed_or_incomplete_selected_artifact_fails_whole_target() {
    let bytes = fixture();
    let input = Input::new(&bytes);
    for bad in [
        b"<html>Rustdoc</html>".as_slice(),
        br#"{"openapi":"3.1.0"}"#,
        br#"{}"#,
    ] {
        assert!(
            assemble(
                config(),
                &[
                    project("valid", &input),
                    project("invalid", &Input::new(bad))
                ],
                Limits::default()
            )
            .is_err()
        );
    }
    let mut broken: Value = serde_json::from_slice(&bytes).expect("fixture");
    broken["children"][0]["signatures"][0]["type"]["target"] = json!(999);
    let broken = serde_json::to_vec(&broken).expect("serialize");
    let error = assemble(
        config(),
        &[project("broken", &Input::new(&broken))],
        Limits::default(),
    )
    .expect_err("dangling reflection");
    assert!(error.to_string().contains("reflection target is absent"));
}

#[test]
fn naming_collisions_and_resource_limits_do_not_silently_shrink() {
    let bytes = fixture();
    let input = Input::new(&bytes);
    assert!(assemble(config(), &[], Limits::default()).is_err());
    assert!(
        assemble(
            config(),
            &[project("same", &input), project("same", &input)],
            Limits::default()
        )
        .is_err()
    );
    for slug in ["../escape", "Upper", "api", "", "a/b", "%2e%2e"] {
        assert!(
            assemble(config(), &[project(slug, &input)], Limits::default()).is_err(),
            "{slug}"
        );
    }
    for limits in [
        Limits {
            entries: 0,
            ..Limits::default()
        },
        Limits {
            total_bytes: 1,
            ..Limits::default()
        },
        Limits {
            depth: 1,
            ..Limits::default()
        },
    ] {
        assert!(assemble(config(), &[project("client", &input)], limits).is_err());
    }
}

#[test]
fn duplicate_reflections_and_missing_group_members_are_rejected() {
    let bytes = fixture();
    let original: Value = serde_json::from_slice(&bytes).expect("fixture");
    let mut duplicate = original.clone();
    duplicate["children"][1]["id"] = json!(1);
    let mut group = original.clone();
    group["groups"] = json!([{"title":"Functions", "children":[1, 400]}]);
    let mut unresolved = original;
    unresolved["children"][0]["signatures"][0]["type"]["target"] = json!(-2);
    for (invalid, reason) in [
        (duplicate, "duplicate TypeDoc reflection id"),
        (group, "reflection target is absent"),
        (unresolved, "unresolved numeric"),
    ] {
        let bytes = serde_json::to_vec(&invalid).expect("JSON");
        let error = assemble(
            config(),
            &[project("client", &Input::new(&bytes))],
            Limits::default(),
        )
        .expect_err("invalid native links");
        assert!(error.to_string().contains(reason), "{error}");
    }
}

#[test]
fn typedoc_profile_and_external_targets_are_explicit() {
    let bytes = fixture();
    let original: Value = serde_json::from_slice(&bytes).expect("fixture");

    let mut wrong_schema = original.clone();
    wrong_schema["schemaVersion"] = json!("3.0");
    let bytes = serde_json::to_vec(&wrong_schema).expect("JSON");
    assert!(
        assemble(
            config(),
            &[project("client", &Input::new(&bytes))],
            Limits::default()
        )
        .expect_err("unqualified TypeDoc schema")
        .to_string()
        .contains("versioned TypeDoc project")
    );

    let mut external = original.clone();
    external["children"][0]["signatures"][0]["type"]["externalUrl"] =
        json!("https://docs.example.invalid/result#value");
    external["comment"] = json!({"summary":[{
        "kind":"inline-tag", "tag":"@link", "text":"Result",
        "target":"https://docs.example.invalid/result#value"
    }]});
    let bytes = serde_json::to_vec(&external).expect("JSON");
    assemble(
        config(),
        &[project("client", &Input::new(&bytes))],
        Limits::default(),
    )
    .expect("explicit uncredentialed external URL remains provider input");

    for target in [json!("../relative"), json!({"qualifiedName":"Result"})] {
        let mut invalid = original.clone();
        invalid["children"][0]["signatures"][0]["type"]["target"] = target;
        let bytes = serde_json::to_vec(&invalid).expect("JSON");
        assert!(
            assemble(
                config(),
                &[project("client", &Input::new(&bytes))],
                Limits::default()
            )
            .expect_err("ambiguous external target")
            .to_string()
            .contains("TypeDoc")
        );
    }
}

#[test]
fn typedoc_unbound_and_symbolic_references_preserve_the_complete_native_payload() {
    let mut model: Value = serde_json::from_slice(&fixture()).expect("fixture");
    let symbol = json!({
        "packageName":"@example/library", "packagePath":"src/model.ts", "qualifiedName":"K"
    });
    model["children"][0]["inheritedFrom"] =
        json!({"type":"reference", "name":"Error.message", "target":-1});
    model["children"][1]["typeParameters"] = json!([{
        "variant":"typeParam", "id":4, "name":"T", "kind":131072, "flags":{}
    }]);
    model["children"][1]["type"] = json!({
        "type":"mapped", "parameter":"K", "readonlyModifier":"+",
        "parameterType":{"type":"typeOperator", "operator":"keyof", "target":{
            "type":"reference", "name":"T", "target":4, "refersToTypeParameter":true
        }},
        "templateType":{"type":"indexedAccess",
            "objectType":{"type":"reference", "name":"T", "target":4},
            "indexType":{"type":"reference", "name":"K", "target":symbol,
                "refersToTypeParameter":true}}
    });
    model["children"]
        .as_array_mut()
        .expect("children")
        .push(json!({
            "variant":"reference", "id":5, "name":"MissingAlias", "kind":4194304,
            "flags":{}, "target":-1
        }));
    model["children"][0]["extendedTypes"] = json!([{
        "type":"reference", "name":"Error", "target":{
            "packageName":"typescript", "packagePath":"lib/lib.es5.d.ts", "qualifiedName":"Error"
        }
    }]);
    model["comment"] = json!({"summary":[
        {"kind":"inline-tag", "tag":"@link", "text":"K", "target":symbol},
        {"kind":"inline-tag", "tag":"@link", "text":"Unresolved authored name"}
    ]});
    let bytes = serde_json::to_vec(&model).expect("JSON");
    let output = assemble(
        config(),
        &[project("client", &Input::new(&bytes))],
        Limits::default(),
    )
    .expect("native unavailable and symbolic references are not fabricated local links");
    assert_eq!(
        output.file(&"artifacts/client/library.json".parse().expect("path")),
        Some(bytes.as_slice()),
        "generics, comments and symbolic identities remain exact"
    );
}

#[test]
fn typedoc_node_unions_do_not_turn_invalid_targets_into_unbound_sentinels() {
    let original: Value = serde_json::from_slice(&fixture()).expect("fixture");
    for target in [
        json!(-2),
        json!(-1.5),
        json!(1.0),
        json!(null),
        json!("https://docs.example.invalid/result"),
    ] {
        let mut model = original.clone();
        model["children"][0]["signatures"][0]["type"]["target"] = target;
        let bytes = serde_json::to_vec(&model).expect("JSON");
        assert!(
            assemble(
                config(),
                &[project("client", &Input::new(&bytes))],
                Limits::default()
            )
            .is_err()
        );
    }
    for target in [
        json!(-2),
        json!(1.5),
        json!({
            "packageName":"example", "packagePath":"index.ts", "qualifiedName":"Value"
        }),
    ] {
        let mut model = original.clone();
        model["children"]
            .as_array_mut()
            .expect("children")
            .push(json!({
                "variant":"reference", "id":4, "name":"Alias", "kind":4194304,
                "flags":{}, "target":target
            }));
        let bytes = serde_json::to_vec(&model).expect("JSON");
        assert!(
            assemble(
                config(),
                &[project("client", &Input::new(&bytes))],
                Limits::default()
            )
            .is_err()
        );
    }
    for target in [json!(-1), json!(-2), json!(0.5), json!(999)] {
        let mut model = original.clone();
        model["comment"] = json!({"summary":[{
            "kind":"inline-tag", "tag":"@link", "text":"Value", "target":target
        }]});
        let bytes = serde_json::to_vec(&model).expect("JSON");
        assert!(
            assemble(
                config(),
                &[project("client", &Input::new(&bytes))],
                Limits::default()
            )
            .is_err()
        );
    }
    for target in [json!(-1), json!(1.5)] {
        let mut model = original.clone();
        model["groups"] = json!([{"title":"Types", "children":[target]}]);
        let bytes = serde_json::to_vec(&model).expect("JSON");
        assert!(
            assemble(
                config(),
                &[project("client", &Input::new(&bytes))],
                Limits::default()
            )
            .is_err()
        );
    }
}

#[test]
fn typedoc_resolved_inline_links_and_aliases_require_existing_local_ids() {
    let original: Value = serde_json::from_slice(&fixture()).expect("fixture");
    for inline in [false, true] {
        let mut model = original.clone();
        if inline {
            model["comment"] = json!({"summary":[{
                "kind":"inline-tag", "tag":"@link", "text":"Result", "target":3
            }]});
        } else {
            model["children"]
                .as_array_mut()
                .expect("children")
                .push(json!({
                    "variant":"reference", "id":4, "name":"Alias", "kind":4194304,
                    "flags":{}, "target":3
                }));
        }
        let bytes = serde_json::to_vec(&model).expect("JSON");
        let output = assemble(
            config(),
            &[project("client", &Input::new(&bytes))],
            Limits::default(),
        )
        .expect("a valid local reference remains supported in each context");
        assert_eq!(
            output.file(&"artifacts/client/library.json".parse().expect("path")),
            Some(bytes.as_slice())
        );
        if !inline {
            model["children"][2]["target"] = json!(999);
            let bytes = serde_json::to_vec(&model).expect("JSON");
            let error = assemble(
                config(),
                &[project("client", &Input::new(&bytes))],
                Limits::default(),
            )
            .expect_err("a positive alias ID must actually resolve");
            assert!(error.to_string().contains("reflection target is absent"));
        }
    }
}

#[test]
fn typedoc_symbol_paths_and_external_urls_keep_their_distinct_refusal_boundaries() {
    let original: Value = serde_json::from_slice(&fixture()).expect("fixture");
    let symbol = json!({
        "packageName":"@example/library", "packagePath":"src/model.ts", "qualifiedName":"Result"
    });
    let mut invalid_symbols = vec![
        json!({"qualifiedName":"Result"}),
        json!({"packageName":"example", "packagePath":7, "qualifiedName":"Result"}),
    ];
    let mut extra_field = symbol.clone();
    extra_field["privatePath"] = json!("/private/model.ts");
    invalid_symbols.push(extra_field);
    for (field, value) in [
        ("packageName", ""),
        ("packageName", "<unknown>"),
        ("packageName", "/private/library"),
        ("packageName", "../library"),
        ("packagePath", ""),
        ("packagePath", "/private/model.ts"),
        ("packagePath", "../model.ts"),
        ("packagePath", "src/../model.ts"),
        ("packagePath", "C:/private/model.ts"),
        ("packagePath", "src\\model.ts"),
        ("packagePath", "%2e%2e/model.ts"),
        ("qualifiedName", ""),
        ("qualifiedName", "Value\nPrivate"),
    ] {
        let mut invalid = symbol.clone();
        invalid[field] = json!(value);
        invalid_symbols.push(invalid);
    }
    for target in invalid_symbols {
        for inline in [false, true] {
            let mut model = original.clone();
            if inline {
                model["comment"] = json!({"summary":[{
                    "kind":"inline-tag", "tag":"@link", "text":"Result", "target":target
                }]});
            } else {
                model["children"][0]["signatures"][0]["type"]["target"] = target.clone();
            }
            let bytes = serde_json::to_vec(&model).expect("JSON");
            assert!(
                assemble(
                    config(),
                    &[project("client", &Input::new(&bytes))],
                    Limits::default()
                )
                .is_err()
            );
        }
    }
    for url in [
        "../relative",
        "javascript:alert(1)",
        "file:///private/model.ts",
        "https://user:secret@docs.example.invalid/result",
        "https://user@docs.example.invalid/",
    ] {
        for inline in [false, true] {
            let mut model = original.clone();
            if inline {
                model["comment"] = json!({"summary":[{
                    "kind":"inline-tag", "tag":"@link", "text":"Result", "target":url
                }]});
            } else {
                model["children"][0]["signatures"][0]["type"]["externalUrl"] = json!(url);
            }
            let bytes = serde_json::to_vec(&model).expect("JSON");
            assert!(
                assemble(
                    config(),
                    &[project("client", &Input::new(&bytes))],
                    Limits::default()
                )
                .is_err()
            );
        }
    }
}

#[test]
fn presentation_values_are_consumed_without_naming_fallbacks() {
    let bytes = fixture();
    let input = Input::new(&bytes);
    let mut selected = config();
    selected.name = "Exact selected brand title";
    selected.primary_color = "#abc";
    selected.theme = "luma";
    let output =
        assemble(selected, &[project("client", &input)], Limits::default()).expect("selected view");
    let configuration: Value = serde_json::from_slice(
        output
            .file(&"docs.json".parse().expect("path"))
            .expect("bytes"),
    )
    .expect("JSON");
    assert_eq!(configuration["name"], "Exact selected brand title");
    assert_eq!(configuration["theme"], "luma");
    assert_eq!(configuration["colors"]["primary"], "#abc");
    for color in ["abc", "#ab", "#xyz", "#12345g"] {
        let mut invalid = config();
        invalid.primary_color = color;
        assert!(assemble(invalid, &[project("client", &input)], Limits::default()).is_err());
    }
}

#[test]
fn duplicate_decoded_json_members_fail_at_every_depth() {
    let bytes = fixture();
    let base = String::from_utf8(bytes).expect("fixture UTF-8");
    for duplicate in [
        base.replacen("{", "{\"name\":\"first\",", 1),
        base.replacen("{", r#"{"\u006eame":"first","#, 1),
        base.replacen("{}", r#"{"duplicate":true,"duplicate":false}"#, 1),
        base.replacen("{}", r#"{"duplicate":true,"\u0064uplicate":false}"#, 1),
    ] {
        let error = assemble(
            config(),
            &[project("client", &Input::new(duplicate.as_bytes()))],
            Limits::default(),
        )
        .expect_err("ambiguous object members");
        assert!(matches!(error, Error::Json(_)));
        assert!(
            error.to_string().contains("duplicate JSON object member"),
            "{error}"
        );
    }
}

#[test]
fn generated_json_is_bounded_while_serializing_with_exact_aggregate_accounting() {
    let bytes = fixture();
    let input = Input::new(&bytes);
    // Quotes expand in JSON; byte checks must use encoded bytes, not source chars.
    let title = "quoted \" value ".repeat(1024);
    let selected = || Config {
        name: &title,
        ..config()
    };
    let modules = [project("client", &input)];
    let baseline =
        assemble(selected(), &modules, Limits::default()).expect("bounded reference tree");
    let exact = baseline.inventory().total_bytes();
    let tree = assemble(
        selected(),
        &modules,
        Limits {
            total_bytes: exact,
            ..Limits::default()
        },
    )
    .expect("exact total including JSON newline, mintignore and provider presentation");
    assert_eq!(tree.inventory(), baseline.inventory());
    let docs_path = "docs.json".parse().expect("path");
    let json_bytes = baseline.file(&docs_path).expect("configuration").len() as u64;
    let aggregate_error = assemble(
        selected(),
        &modules,
        Limits {
            total_bytes: exact - 1,
            ..Limits::default()
        },
    )
    .expect_err("all target files, including provider presentation, share the aggregate ceiling");
    assert!(matches!(
        aggregate_error,
        Error::Tree(gazette_core::source::Error::Limit { .. })
    ));
    for limits in [
        Limits {
            total_bytes: bytes.len() as u64 + 100,
            ..Limits::default()
        },
        Limits {
            file_bytes: json_bytes - 1,
            ..Limits::default()
        },
    ] {
        let error = assemble(selected(), &modules, limits)
            .expect_err("serialization must stop at its byte ceiling");
        assert!(
            matches!(error, Error::Tree(
            gazette_core::source::Error::Limit { ref path, resource: "file or total bytes during JSON serialization" }
        ) if path == std::path::Path::new("docs.json")),
            "{error}"
        );
    }
}

#[test]
fn retained_tree_selection_is_exact_without_copying_siblings_or_inventing_format_support() {
    use gazette_core::{
        documentation::{Artifact, Selection},
        source::{CapturedSource, EntryPath, InventoryBuilder, Sha256Digest},
    };
    use std::collections::BTreeMap;

    let bytes = fixture();
    let selected: EntryPath = "native/reference.json".parse().expect("selected file");
    let sibling: EntryPath = "native/private.txt".parse().expect("unselected sibling");
    let directory: EntryPath = "native".parse().expect("directory");
    let missing: EntryPath = "missing.json".parse().expect("absent entry");
    let files = BTreeMap::from([
        (selected.clone(), bytes.clone()),
        (sibling.clone(), b"not a selected reference".to_vec()),
    ]);
    let mut inventory = InventoryBuilder::new(Limits::default());
    inventory
        .insert(directory.clone(), Content::Directory)
        .expect("native directory");
    for (path, data) in &files {
        inventory
            .insert(
                path.clone(),
                Content::File {
                    bytes: data.len() as u64,
                    sha256: Sha256Digest::of(data),
                },
            )
            .expect("native file");
    }
    let input = CapturedSource::from_parts(inventory.finish(), files).expect("retained tree");
    let selected_file = Selection::File {
        path: selected.clone(),
    };
    let invalid_selections = [
        Selection::File {
            path: directory.clone(),
        },
        Selection::File { path: missing },
        Selection::Directory {
            path: Some(directory),
        },
        Selection::Directory { path: None },
    ];
    let project = |selection, format| ProjectModule {
        slug: "client",
        title: "Client",
        manual: None,
        navigation: vec![ProjectItem::Reference(Reference {
            pages: None,
            slug: "library",
            title: "Library",
            artifact: Artifact {
                format,
                tree: input.as_input(),
                selection,
            },
        })],
    };
    let output = assemble(
        config(),
        &[project(&selected_file, Format::TypeDocJson)],
        Limits::default(),
    )
    .expect("only the selected native file is consumed");
    assert_eq!(
        output.file(&"artifacts/client/library.json".parse().expect("output")),
        Some(bytes.as_slice())
    );
    assert!(output.file(&sibling).is_none());
    assert_eq!(
        output
            .inventory()
            .entries()
            .iter()
            .filter(|entry| matches!(entry.content, Content::File { .. }))
            .count(),
        4,
        "only docs.json, mintignore, provider presentation and the selected artifact"
    );
    for selection in &invalid_selections {
        let error = assemble(
            config(),
            &[project(selection, Format::TypeDocJson)],
            Limits::default(),
        )
        .expect_err("a tree is not permission to substitute another file");
        assert_eq!(error.code(), "documentation_target_input");
    }
    for (format, reason) in [
        (
            Format::RustdocHtml,
            "Rustdoc HTML requires a selected native directory",
        ),
        (Format::RustdocJson, "unsupported native Rustdoc format"),
    ] {
        let error = assemble(
            config(),
            &[project(&selected_file, format)],
            Limits::default(),
        )
        .expect_err("a TypeDoc file cannot substitute for the selected Rustdoc representation");
        assert_eq!(error.code(), "documentation_target_input");
        assert!(error.to_string().contains(reason), "{error}");
    }
    assert_eq!(input.file(&selected), Some(bytes.as_slice()));
    assert_eq!(
        input.file(&sibling),
        Some(b"not a selected reference".as_slice())
    );
}
