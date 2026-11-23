//! Discovery schemas and calls exercise the same native workspace query owners.

use std::fs;

use crate::configuration::Route;
use gazette_cli::sdk::{configuration as sdk_configuration, source, toml, workspace};
use serde_json::{Value, json};
use tempfile::tempdir;

const SOURCE: &str = "[composition.capabilities]\n'publication.feeds/v1'={state='disabled'}\n[composition.resources]\n'module:feeds'={state='disabled'}\n[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Rêve'\n";

#[test]
fn workspace_check_schema_and_decoder_keep_local_and_supplied_requests_distinct() {
    let schema = schema(Route::Check);
    let validator = jsonschema::validator_for(&schema).unwrap();
    for input in [
        json!({}),
        json!({"workspace":"/publication"}),
        json!({"source":SOURCE}),
        json!({"source":SOURCE,"limits":{}}),
    ] {
        assert!(validator.is_valid(&input), "{input}");
        assert!(Route::Check.decode(arguments(input.clone())).is_ok());
        let mut extra = input;
        extra["unknown"] = json!(true);
        assert!(!validator.is_valid(&extra), "{extra}");
        assert!(Route::Check.decode(arguments(extra)).is_err());
    }
    for input in [
        json!({"source":null}),
        json!({"workspace":17}),
        json!({"limits":{}}),
        json!({"workspace":"/publication","source":SOURCE}),
        json!({"workspace":"/publication","limits":{}}),
    ] {
        assert!(!validator.is_valid(&input), "{input}");
        assert!(Route::Check.decode(arguments(input)).is_err());
    }
}

#[test]
fn native_unsigned_ranges_are_exact_in_limits_and_both_query_paths() {
    let beyond: Value = serde_json::from_str(&((usize::MAX as u128) + 1).to_string())
        .expect("exact out-of-range JSON integer");
    let check_schema = schema(Route::Check);
    let validator = jsonschema::validator_for(&check_schema).expect("limits schema");
    for field in ["source_bytes", "nodes", "depth"] {
        for (value, accepted) in [(json!(usize::MAX), true), (beyond.clone(), false)] {
            let input = json!({"source":SOURCE,"limits":{field:value}});
            assert_eq!(validator.is_valid(&input), accepted, "{input}");
            assert_eq!(Route::Check.decode(arguments(input)).is_ok(), accepted);
        }
    }
    for (route, query, path) in [
        (
            Route::Field,
            json!({"kind":"brand","identity":"brand:one","field":[]}),
            "field",
        ),
        (
            Route::Composition,
            json!({"site":"site:one","field":{"kind":"theme"},"path":[]}),
            "path",
        ),
    ] {
        let schema = schema(route);
        let validator = jsonschema::validator_for(&schema).expect("query schema");
        for (value, accepted) in [(json!(usize::MAX), true), (beyond.clone(), false)] {
            let mut query = query.clone();
            query[path] = json!([value]);
            let input = json!({"source":SOURCE,"query":query});
            assert_eq!(validator.is_valid(&input), accepted, "{input}");
            assert_eq!(route.decode(arguments(input)).is_ok(), accepted);
        }
    }
}

fn arguments(value: Value) -> rmcp::model::JsonObject {
    value.as_object().expect("named arguments").clone()
}

fn execute(route: Route, input: Value) -> rmcp::model::CallToolResult {
    let result = route
        .decode(arguments(input))
        .expect("transport input")
        .execute(1 << 20)
        .expect("native call encoding");
    let schema =
        serde_json::to_value(route.descriptor()).expect("descriptor")["outputSchema"].clone();
    jsonschema::validator_for(&schema)
        .expect("complete native result schema")
        .validate(
            result
                .structured_content
                .as_ref()
                .expect("structured native result"),
        )
        .expect("actual emitted result matches advertised schema");
    result
}

fn schema(route: Route) -> Value {
    serde_json::to_value(route.descriptor()).expect("tool descriptor")["inputSchema"].clone()
}

#[test]
fn descriptors_expose_native_query_variants_paths_and_defaulted_limits() {
    for route in Route::ALL {
        let descriptor = serde_json::to_value(route.descriptor()).expect("descriptor");
        assert_eq!(descriptor["name"], route.name());
        assert_eq!(descriptor["annotations"]["readOnlyHint"], true);
        assert_eq!(descriptor["annotations"]["destructiveHint"], false);
        assert_eq!(descriptor["annotations"]["openWorldHint"], false);
        let schema = schema(route);
        let validator = jsonschema::validator_for(&schema).expect("complete local schema");
        let query = match route {
            Route::Check => None,
            Route::Load => None,
            Route::Schema => None,
            Route::Field => Some(
                json!({"kind":"brand","identity":"brand:one","field":["publications",0,"site"]}),
            ),
            Route::Composition => Some(json!({"site":"site:one","field":{"kind":"theme"}})),
        };
        let mut input = match route {
            Route::Load => json!({
                "allow_native_reads":true,
                "root":"/explicit/configuration",
                "workspace":"gazette.toml"
            }),
            Route::Schema => json!({"contract":"theme-manifest"}),
            _ => json!({"source":SOURCE}),
        };
        if let Some(query) = query {
            input["query"] = query;
        }
        assert!(validator.is_valid(&input));
        if matches!(route, Route::Schema) {
            assert!(route.decode(arguments(input)).is_ok());
            continue;
        }
        input["limits"] = if matches!(route, Route::Load) {
            json!({"source":{"entries":3}})
        } else {
            json!({"source_bytes":SOURCE.len(),"nodes":1000,"depth":16})
        };
        assert!(validator.is_valid(&input));
        assert!(route.decode(arguments(input.clone())).is_ok());
        input["limits"]["unexpected"] = json!(true);
        assert!(!validator.is_valid(&input));
        assert!(route.decode(arguments(input)).is_err());
    }
    let field = schema(Route::Field);
    let validator = jsonschema::validator_for(&field).expect("field schema");
    for kind in [
        "brand",
        "principal",
        "site",
        "project",
        "taxonomy",
        "source",
    ] {
        let query = json!({"kind":kind,"identity":"owner:one","field":["literal.dot",0]});
        assert!(serde_json::from_value::<workspace::FieldQuery>(query.clone()).is_ok());
        assert!(validator.is_valid(&json!({"source":SOURCE,"query":query})));
    }
    let composition = schema(Route::Composition);
    let validator = jsonschema::validator_for(&composition).expect("composition schema");
    for field in [
        json!({"kind":"theme"}),
        json!({"kind":"capability","capability":"publication.feeds/v1"}),
        json!({"kind":"resource","resource":"module:feeds"}),
    ] {
        let query = json!({"site":"site:one","field":field,"path":["source",0]});
        assert!(serde_json::from_value::<workspace::CompositionOriginQuery>(query.clone()).is_ok());
        assert!(validator.is_valid(&json!({"source":SOURCE,"query":query})));
    }
}

#[test]
fn query_schema_and_native_decoder_reject_wrong_catalog_shapes() {
    for (route, queries) in [
        (
            Route::Field,
            vec![
                json!({"kind":"unknown","identity":"brand:one","field":[]}),
                json!({"kind":"brand","identity":"brand:one","field":[-1]}),
                json!({"kind":"brand","identity":"brand:one","field":[1.5]}),
                json!({"kind":"brand","identity":"brand:one","field":[{}]}),
                json!({"kind":"brand","identity":"brand:one","field":[],"other":1}),
                json!(["brand", "brand:one", []]),
            ],
        ),
        (
            Route::Composition,
            vec![
                json!({"site":"site:one","field":{"kind":"theme","capability":"cap:x"}}),
                json!({"site":"site:one","field":{"kind":"capability"}}),
                json!({"site":"site:one","field":{"kind":"resource"}}),
                json!({"site":"site:one","field":{"kind":"module","module":"module:feeds"}}),
                json!({"site":"site:one","field":{"kind":"unknown"}}),
                json!({"site":"site:one","field":{"kind":"theme"},"path":[true]}),
                json!(["site:one",{"kind":"theme"},[]]),
            ],
        ),
    ] {
        let schema = schema(route);
        let validator = jsonschema::validator_for(&schema).expect("query schema");
        for query in queries {
            let input = json!({"source":SOURCE,"query":query});
            assert!(!validator.is_valid(&input), "{input}");
            assert!(route.decode(arguments(input.clone())).is_err(), "{input}");
        }
    }
    for limits in [
        json!([]),
        json!([1, 2, 3]),
        json!(null),
        json!({"depth":-1}),
        json!({"nodes":false}),
    ] {
        assert!(
            Route::Check
                .decode(arguments(json!({"source":SOURCE,"limits":limits})))
                .is_err()
        );
    }
}

#[test]
fn key_discovery_matches_native_unicode_whitespace_and_control_grammar() {
    let schema = schema(Route::Field);
    let validator = jsonschema::validator_for(&schema).expect("native key schema");
    for key in [
        "",
        ":",
        "::",
        ":a",
        "a:",
        "a:b",
        "a::",
        "a:b:c",
        "é:日本",
        "a: b",
        "a:\u{0}",
        "a:\u{7f}",
        "a:\u{85}",
        "a:\u{a0}",
        "a:\u{1680}",
        "a:\u{2000}",
        "a:\u{200a}",
        "a:\u{2028}",
        "a:\u{2029}",
        "a:\u{202f}",
        "a:\u{205f}",
        "a:\u{3000}",
        "a:\u{200b}",
        "a:\u{feff}",
    ] {
        let native = serde_json::from_value::<workspace::Key>(json!(key)).is_ok();
        let input = json!({"source":SOURCE,"query":{"kind":"brand","identity":key,"field":[]}});
        assert_eq!(validator.is_valid(&input), native, "key {key:?}");
        assert_eq!(
            Route::Field.decode(arguments(input)).is_ok(),
            native,
            "key {key:?}"
        );
    }
}

#[test]
fn configuration_calls_preserve_exact_sdk_outputs_and_native_failures() {
    let parsed = workspace::ParsedWorkspace::parse(SOURCE, Default::default()).expect("workspace");
    let checked = execute(Route::Check, json!({"source":SOURCE}));
    assert_eq!(checked.is_error, Some(false));
    assert_eq!(
        checked.structured_content,
        Some(serde_json::to_value(parsed.compilation()).expect("SDK result"))
    );
    let field = json!({"kind":"brand","identity":"brand:one","field":["name"]});
    let expected = parsed
        .origin(&serde_json::from_value(field.clone()).expect("native query"))
        .expect("native origin");
    assert_eq!(&SOURCE[expected.span.start..expected.span.end], "'Rêve'");
    assert_eq!(
        execute(Route::Field, json!({"source":SOURCE,"query":field})).structured_content,
        Some(serde_json::to_value(expected).expect("SDK origin"))
    );
    let composition = json!({"site":"site:one","field":{"kind":"capability","capability":"publication.feeds/v1"},"path":["state"]});
    let expected = parsed
        .composition_origin(&serde_json::from_value(composition.clone()).expect("native query"))
        .expect("native composition origin");
    assert_eq!(
        execute(
            Route::Composition,
            json!({"source":SOURCE,"query":composition})
        )
        .structured_content,
        Some(serde_json::to_value(expected).expect("SDK composition origin"))
    );
    let resource = json!({"site":"site:one","field":{"kind":"resource","resource":"module:feeds"},"path":["state"]});
    let expected = parsed
        .composition_origin(&serde_json::from_value(resource.clone()).expect("native query"))
        .expect("native resource origin");
    assert_eq!(
        expected.selection.field,
        workspace::SelectionField::Resource {
            resource: "module:feeds".parse().expect("resource")
        }
    );
    assert_eq!(
        execute(
            Route::Composition,
            json!({"source":SOURCE,"query":resource})
        )
        .structured_content,
        Some(serde_json::to_value(expected).expect("SDK resource origin"))
    );
    let failed = execute(
        Route::Field,
        json!({"source":SOURCE,"query":{"kind":"brand","identity":"brand:missing","field":[]}}),
    );
    assert_eq!(failed.is_error, Some(true));
    assert_eq!(
        failed.structured_content.expect("failure")["code"],
        "origin_unknown_identity"
    );
}

#[test]
fn contract_schema_call_projects_each_native_typed_owner() {
    for contract in [
        "theme-manifest",
        "template-context",
        "workspace",
        "authored-content",
    ] {
        let result = execute(Route::Schema, json!({"contract":contract}));
        assert_eq!(result.is_error, Some(false));
        let schema = result.structured_content.expect("schema");
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(schema["type"], "object");
    }
}

#[test]
fn native_workspace_call_returns_exact_bounded_inventory_and_compiled_documents() {
    let fixture = tempdir().expect("workspace root");
    fs::create_dir(fixture.path().join("sites")).expect("site directory");
    fs::write(
        fixture.path().join("gazette.toml"),
        "includes=['sites/one.toml']\n[[principals]]\nid='owner:one'\nkind='person'\n",
    )
    .expect("root document");
    fs::write(
        fixture.path().join("sites/one.toml"),
        "[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n",
    )
    .expect("included document");
    fs::write(fixture.path().join("unselected.toml"), b"\xff").expect("invalid unselected sibling");
    let workspace: source::EntryPath = "gazette.toml".parse().expect("entry path");
    let native = sdk_configuration::load_workspace(
        fixture.path(),
        &workspace,
        sdk_configuration::WorkspaceLoadLimits::default(),
    )
    .expect("shared SDK result");
    let result = execute(
        Route::Load,
        json!({
            "allow_native_reads":true,
            "root":fixture.path(),
            "workspace":"gazette.toml"
        }),
    );
    assert_eq!(result.is_error, Some(false));
    assert_eq!(
        result.structured_content,
        Some(serde_json::to_value(native).expect("SDK evidence"))
    );

    let denied = execute(
        Route::Load,
        json!({
            "allow_native_reads":false,
            "root":fixture.path().join("missing"),
            "workspace":"gazette.toml"
        }),
    );
    assert_eq!(denied.is_error, Some(true));
    assert_eq!(
        denied.structured_content.expect("typed failure")["code"],
        "workspace_load_permission"
    );
}

#[test]
fn each_parser_ceiling_uses_native_defaults_and_fails_without_partial_output() {
    let defaults = toml::Limits::default();
    let omitted: toml::Limits = serde_json::from_value(json!({})).expect("native defaults");
    assert_eq!(
        (omitted.source_bytes, omitted.nodes, omitted.depth),
        (defaults.source_bytes, defaults.nodes, defaults.depth)
    );
    let exact = execute(
        Route::Check,
        json!({"source":SOURCE,"limits":{"source_bytes":SOURCE.len()}}),
    );
    assert_eq!(exact.is_error, Some(false));
    for limits in [
        json!({"source_bytes":SOURCE.len()-1}),
        json!({"nodes":0}),
        json!({"depth":0}),
    ] {
        let native: toml::Limits = serde_json::from_value(limits.clone()).expect("native limits");
        let error = workspace::ParsedWorkspace::parse(SOURCE, native).expect_err("native bound");
        let expected =
            serde_json::to_value(workspace::Failure::from(&error)).expect("native failure");
        let result = execute(Route::Check, json!({"source":SOURCE,"limits":limits}));
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.structured_content, Some(expected));
    }
}

#[test]
fn serialized_result_schemas_preserve_native_structure_inheritance_and_failures() {
    let mut source = include_str!("../../../../sdk/js/tests/fixtures/workspace.toml").to_owned();
    source.push_str(r#"
[[projects]]
id='project:one'
lifecycle='done'
visibility='unlisted'
availability='gated'
[[taxonomies]]
id='taxonomy:topics'
label='Topics'
language='en-US'
terms=[{id='term:one',label='One'}]
[[brands.publications]]
site='site:a'
public_base_url='https://example.invalid/docs/'
authored=[{source='source:theme',target='articles',static_assets={route_prefix='/assets/'}}]
documentation_repository={name='site-reference',path='/not-read/synthetic/reference',build={target={kind='mintlify',theme='mint',primary_color='#123456'},modules=[{project='project:one',slug='one',title='One',navigation=[{kind='reference',slug='api',title='API',input={kind='artifact',source='source:theme',selection={kind='file',path='api.json'},format='openapi-json'}}]}]}}
machine={sitemap=true,llms={enabled=true,description_from='content:one',representation='markdown-alternate',include_human_link=true,sections=[{title='Start here',contents=['content:one']}]},markdown_alternates={enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'},search='lexical',json_ld=true,feeds={identity_base='https://example.invalid/ids/',channels=[{id='feed:one',channel='content:one',path='/feed.xml',format='atom',scope={kind='collection',recursive=true}}]}}
[[placements]]
role = 'primary'
site='site:a'
project='project:one'
render=true
discoverable=false
documents={source='source:theme',target='projects/one'}
native_markdown={route_prefix='/projects/one/',lifecycle='archived',language='en-US'}
"#);
    let result = execute(Route::Check, json!({"source":source}));
    assert_eq!(result.is_error, Some(false), "{result:?}");
    let native = workspace::compile(&source, Default::default()).expect("complete fixture");
    let value = result.structured_content.expect("native compilation");
    assert_eq!(value, json!(native));
    let schema =
        serde_json::to_value(Route::Check.descriptor()).expect("descriptor")["outputSchema"]
            .clone();
    let validator = jsonschema::validator_for(&schema).expect("serialization schema");
    for (pointer, wrong) in [
        ("/source_sha256", json!("not-a-hash")),
        (
            "/workspace/projects/project:one/availability",
            json!("public"),
        ),
        ("/workspace/principals/owner:one/kind", json!("site")),
        (
            "/workspace/brands/brand:one/publications/0/public_base_url",
            json!({"url":"https://example.invalid/"}),
        ),
        (
            "/workspace/placements/0/native_markdown/route_prefix",
            json!({"url":"/projects/one/"}),
        ),
        (
            "/workspace/site_compositions/site:a/theme/origin/scope",
            json!("unknown"),
        ),
    ] {
        let mut changed = value.clone();
        *changed
            .pointer_mut(pointer)
            .expect("real nonempty nested path") = wrong;
        assert!(!validator.is_valid(&changed), "wrong {pointer}");
    }
    let mut absent = value.clone();
    absent["workspace"]
        .as_object_mut()
        .expect("workspace")
        .remove("projects");
    assert!(
        !validator.is_valid(&absent),
        "serialized collections are required even when empty"
    );
    for bad in [
        "sites=[]",
        "PRIVATE=[",
        "[[sites]]\nid='site:one'\npublisher='owner:missing'\ndeployable=false",
    ] {
        assert_eq!(
            execute(Route::Check, json!({"source":bad})).is_error,
            Some(true)
        );
    }
    let limited = Route::Check
        .decode(arguments(json!({"source":source})))
        .expect("request")
        .execute(1)
        .expect("bounded result");
    assert_eq!(limited.is_error, Some(true));
    assert!(validator.is_valid(limited.structured_content.as_ref().expect("limit record")));
}
