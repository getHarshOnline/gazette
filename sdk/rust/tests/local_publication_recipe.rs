//! The native request owner rejects invalid shape before any source acquisition.

use gazette_sdk::{build::LocalPublicationRecipe, source};
use serde_json::{Value, json};

const WORKSPACE: &str = "[[principals]]\nid='person:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='person:one'\ndeployable=false\n[[sites]]\nid='site:other'\npublisher='person:one'\ndeployable=false\n";

fn input() -> Value {
    json!({"format":"gazette-local-publication-set/1","workspace":{"kind":"supplied","source":WORKSPACE},
        "sites":[{"site":"site:one","model_output":"model.json"}]})
}

fn parse(value: &Value) -> Result<LocalPublicationRecipe, &'static str> {
    LocalPublicationRecipe::parse_json(value.to_string().as_bytes(), 8192, 2)
}

#[test]
fn discovery_and_native_parser_share_optional_candidate_and_site_shapes() {
    let schema = serde_json::to_value(LocalPublicationRecipe::input_schema()).expect("schema");
    let validator = jsonschema::validator_for(&schema).expect("closed discovery schema");
    for candidate in [false, true] {
        for explicit_nulls in [false, true] {
            let mut value = input();
            if candidate {
                value["selection"] = json!({"content":{"article:one":"exclude"},"assets":{}});
            }
            if explicit_nulls {
                value["sites"][0]["graph"] = Value::Null;
            }
            validator
                .validate(&value)
                .expect("discoverable native request");
            let parsed = parse(&value).expect("shared native request");
            assert!(
                matches!(parsed.workspace(), gazette_sdk::build::WorkspaceInput::Supplied { source } if source == WORKSPACE)
            );
            assert_eq!(
                parsed
                    .invocation_sites()
                    .expect("explicit override")
                    .iter()
                    .map(|site| site.site.to_string())
                    .collect::<Vec<_>>(),
                ["site:one"]
            );
            let resolved = parsed
                .resolve(Default::default())
                .expect("valid supplied workspace, no source acquisition");
            let options = resolved.options(source::Limits::default());
            assert_eq!(options.selection.is_some(), candidate);
            assert_eq!(options.sites.len(), 1);
            assert_eq!(
                options
                    .sites
                    .values()
                    .next()
                    .expect("site")
                    .model_output
                    .expect("explicit model output")
                    .as_str(),
                "model.json"
            );
            assert!(options.preview_urls.is_none());
        }
    }
}

#[test]
fn unknown_null_and_positional_shapes_are_refused_by_native_and_discovery() {
    let schema = serde_json::to_value(LocalPublicationRecipe::input_schema()).expect("schema");
    let validator = jsonschema::validator_for(&schema).expect("schema");
    let mut cases = vec![Value::Null, json!([])];
    for (pointer, replacement) in [
        ("/selection", Value::Null),
        ("/selection", json!([{}, {}])),
        ("/selection", json!({"content":[],"assets":{}})),
        (
            "/selection",
            json!({"content":{"wrong identity":"include"},"assets":{}}),
        ),
        (
            "/selection",
            json!({"content":{"content:bad name":"include"},"assets":{}}),
        ),
        (
            "/selection",
            json!({"content":{},"assets":{"../private":"include"}}),
        ),
        (
            "/selection",
            json!({"content":{},"assets":{},"unknown":true}),
        ),
        ("/sites/0", json!(["site:one", "model.json", null])),
        ("/sites", Value::Null),
        ("/sites/0/graph", json!(["graph/index.html"])),
        ("/sites/0/graph", json!({"output":"../escape"})),
        ("/format", json!("gazette-publication-set/unknown")),
        ("/format", json!({"gazette-local-publication-set/1":null})),
    ] {
        let mut value = input();
        let (parent, field) = pointer.rsplit_once('/').expect("field");
        let parent = value.pointer_mut(parent).expect("parent");
        if let Some(array) = parent.as_array_mut() {
            array[field.parse::<usize>().expect("array index")] = replacement;
        } else {
            parent[field] = replacement;
        }
        cases.push(value);
    }
    let mut unknown = input();
    unknown["unknown"] = json!(true);
    cases.push(unknown);
    for value in cases {
        assert!(parse(&value).is_err(), "native accepted {value}");
        assert!(!validator.is_valid(&value), "discovery accepted {value}");
    }
}

#[test]
fn duplicate_decoded_keys_and_complete_request_bounds_are_enforced() {
    let input = input().to_string();
    let exact = input.len();
    assert!(LocalPublicationRecipe::parse_json(input.as_bytes(), exact, 1).is_ok());
    assert!(LocalPublicationRecipe::parse_json(input.as_bytes(), exact - 1, 1).is_err());
    assert!(LocalPublicationRecipe::parse_json(input.as_bytes(), exact, 0).is_err());
    for invalid in [
        input.replacen("\"format\":", "\"format\":\"gazette-local-publication-set/1\",\"format\":", 1),
        input.replacen("\"site\":", "\"s\\u0069te\":\"site:one\",\"site\":", 1),
        input.replacen("\"workspace\":", "\"selection\":{\"content\":{\"article:one\":\"include\",\"article:one\":\"exclude\"},\"assets\":{}},\"workspace\":", 1),
    ] {
        assert!(LocalPublicationRecipe::parse_json(invalid.as_bytes(), 8192, 2).is_err());
    }
}

#[test]
fn explicit_site_membership_cannot_be_empty_or_duplicated() {
    let mut value = input();
    value["sites"] = json!([]);
    assert!(parse(&value).is_err());
    let mut value = input();
    value["sites"] = json!([value["sites"][0], value["sites"][0]]);
    assert!(parse(&value).is_err());
    value["sites"][1]["site"] = json!("site:other");
    let recipe = parse(&value).expect("two explicit sites");
    assert_eq!(
        recipe
            .invocation_sites()
            .expect("explicit override")
            .iter()
            .map(|site| site.site.to_string())
            .collect::<Vec<_>>(),
        ["site:one", "site:other"]
    );
}

#[test]
fn omitted_sites_are_pure_unresolved_intent_and_null_is_not_omission() {
    let directory = tempfile::tempdir().expect("disposable explicit root parent");
    let missing = directory.path().join("never-created");
    let value = json!({"format":"gazette-local-publication-set/1",
        "workspace":{"kind":"local","root":missing,"workspace":"gazette.toml"}});
    let schema = serde_json::to_value(LocalPublicationRecipe::input_schema()).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(validator.is_valid(&value));
    let unresolved = parse(&value).expect("pure parse does not open missing native root");
    assert!(unresolved.invocation_sites().is_none());
    assert!(!missing.exists());
    let mut null = value.clone();
    null["sites"] = Value::Null;
    assert!(!validator.is_valid(&null));
    assert!(parse(&null).is_err());
    let mut empty = value;
    empty["sites"] = json!([]);
    assert!(
        parse(&empty).is_err(),
        "empty override does not select defaults"
    );
}

#[test]
fn configured_resolution_preserves_order_owner_and_draft_policy_without_acquisition() {
    use gazette_sdk::workspace::{BuildOrigin, LocalDrafts};
    let source =
        format!("[build]\nsites=['site:other','site:one']\nlocal_drafts='exclude'\n{WORKSPACE}");
    let value = json!({"format":"gazette-local-publication-set/1",
        "workspace":{"kind":"supplied","source":source}});
    let unresolved = parse(&value).unwrap();
    assert!(unresolved.invocation_sites().is_none());
    let resolved = unresolved
        .resolve(Default::default())
        .expect("configuration-only resolution");
    assert_eq!(
        resolved
            .sites()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["site:other", "site:one"]
    );
    assert_eq!(resolved.build_selection().origin, BuildOrigin::Workspace);
    assert_eq!(
        resolved.build_selection().local_drafts,
        Some(LocalDrafts::Exclude)
    );
    assert!(
        resolved
            .options(Default::default())
            .sites
            .values()
            .all(|site| site.model_output.is_none())
    );
    let mut explicit = value;
    explicit["sites"] = json!([{"site":"site:one","model_output":"debug.json"}]);
    let explicit = parse(&explicit)
        .unwrap()
        .resolve(Default::default())
        .unwrap();
    assert_eq!(
        explicit
            .sites()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["site:one"]
    );
    assert_eq!(explicit.build_selection().origin, BuildOrigin::Invocation);
    assert_eq!(
        explicit.build_selection().local_drafts,
        Some(LocalDrafts::Exclude)
    );
    assert_eq!(
        explicit
            .options(Default::default())
            .sites
            .values()
            .next()
            .unwrap()
            .model_output
            .unwrap()
            .as_str(),
        "debug.json"
    );
}

#[test]
fn missing_or_oversized_configured_defaults_fail_during_resolution_not_parse() {
    let mut value = input();
    value.as_object_mut().unwrap().remove("sites");
    let missing = parse(&value).expect("missing defaults are not a JSON shape error");
    assert_eq!(
        missing.resolve(Default::default()).unwrap_err().code(),
        "publication_set_selection"
    );
    value["workspace"]["source"] = json!(format!(
        "[build]\nsites=['site:one','site:other']\nlocal_drafts='include'\n{WORKSPACE}"
    ));
    let limited = LocalPublicationRecipe::parse_json(value.to_string().as_bytes(), 8192, 1)
        .expect("effective configured count is not known before configuration");
    assert_eq!(
        limited.resolve(Default::default()).unwrap_err().code(),
        "publication_set_selection"
    );
    let resolved = parse(&value).unwrap().resolve(Default::default()).unwrap();
    assert_eq!(
        resolved.sites().count(),
        2,
        "never silently shrink defaults to fit"
    );
}

#[test]
fn configured_corpus_and_math_keep_invocation_limits_and_permission_distinct() {
    use gazette_sdk::{build, corpus};
    let mut request = input();
    request["workspace"]["source"] = json!(format!(
        "[build]\nsites=['site:one']\nlocal_drafts='include'\ncorpus_bytes=536870912\nmath=true\n{WORKSPACE}"
    ));
    let resolved = parse(&request)
        .unwrap()
        .resolve(Default::default())
        .unwrap();
    let defaults = resolved.render_limits(Default::default());
    assert_eq!(defaults.publication.corpus.source_bytes, 536870912);
    assert_eq!(
        defaults.publication.corpus.markdown_nodes,
        corpus::Limits::default().markdown_nodes
    );
    assert_eq!(defaults.bytes, 1 << 30);
    let math = resolved.math_generation().unwrap();
    assert!(!math.execution.allow_execute);
    assert_eq!(math.execution.max_input_bytes, 2 << 20);
    assert_eq!(math.formulas, 4096);
    assert_eq!(math.output_bytes, 64 << 20);

    let mut explicit = build::LocalRenderLimits::default();
    explicit.publication.corpus.source_bytes = Some(16 << 20);
    explicit.publication.corpus.markdown_nodes = 7;
    let limits = resolved.render_limits(explicit);
    assert_eq!(limits.publication.corpus.source_bytes, 16 << 20);
    assert_eq!(limits.publication.corpus.markdown_nodes, 7);
    explicit.publication.corpus.source_bytes = Some(768 << 20);
    assert_eq!(
        resolved
            .render_limits(explicit)
            .publication
            .corpus
            .source_bytes,
        768 << 20
    );

    let permitted = parse(&request)
        .unwrap()
        .with_math_execution(None, true)
        .unwrap()
        .resolve(Default::default())
        .unwrap();
    assert!(permitted.math_generation().unwrap().execution.allow_execute);
    let off = parse(&request)
        .unwrap()
        .with_math_execution(Some(false), true)
        .unwrap()
        .resolve(Default::default())
        .unwrap();
    assert!(off.math_generation().is_none());
    request["math_generation"] = json!({"allow_execute":true});
    assert!(
        parse(&request)
            .unwrap()
            .resolve(Default::default())
            .unwrap()
            .math_generation()
            .unwrap()
            .execution
            .allow_execute
    );
    request["math"] = json!(false);
    assert!(parse(&request).is_err());
}

#[test]
fn omitted_workspace_build_limits_keep_native_bounds_and_partial_corpus_input() {
    use gazette_sdk::{build, corpus};
    let resolved = parse(&input())
        .unwrap()
        .resolve(Default::default())
        .unwrap();
    assert_eq!(
        resolved
            .render_limits(Default::default())
            .publication
            .corpus
            .source_bytes,
        corpus::Limits::default().source_bytes
    );
    assert!(resolved.math_generation().is_none());
    let partial: build::CorpusLimits = serde_json::from_value(json!({"files":1})).unwrap();
    assert!(partial.source_bytes.is_none());
    assert_eq!(partial.files, 1);
    let concrete = build::CorpusLimits::from(corpus::Limits {
        source_bytes: 17,
        ..Default::default()
    });
    assert_eq!(concrete.source_bytes, Some(17));
}
